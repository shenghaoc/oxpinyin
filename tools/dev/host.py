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


def linux_cgroup_dirs(proc=Path('/proc'), root=Path('/sys/fs/cgroup'), *, controller=None):
    """Find the current cgroup and ancestors up to the visible mount root.

    The default retains the v2 unified path. V1 controller mounts can live
    anywhere and can expose only a subtree of the controller hierarchy.
    """
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


def _cgroup_v1_cpu(directory, label, notes, cpus):
    """Append a finite v1 quota/period CPU capacity and diagnostic notes.

    Treat -1 as unlimited and record unreadable or invalid values as unknown.
    """
    try:
        quota = int((directory / 'cpu.cfs_quota_us').read_text().strip())
        if quota == -1:
            notes.append(f'{label} cpu.cfs_quota_us=-1 (unlimited)')
            return
        period = int((directory / 'cpu.cfs_period_us').read_text().strip())
        if quota <= 0 or period <= 0:
            raise ValueError('invalid CPU quota or period')
        cpus.append(quota / period)
        notes.append(f'{label} cpu.cfs_quota_us={quota}; cpu.cfs_period_us={period}')
    except (OSError, ValueError):
        notes.append(f'{label} CPU quota=unknown')


def cpu_snapshot(proc: Path = Path('/proc'), cgroup_root: Path = Path('/sys/fs/cgroup')) -> dict[str, object]:
    """Return the CPU budget with diagnostics for unreadable limits.

    CPU budgets combine affinity and cgroup quotas: cpus floors and
    cargo_cpus ceils the minimum, with a lower bound of one.
    """
    notes, cpus = [], []
    if sys.platform.startswith('linux'):
        try:
            affinity = len(allowed_cpus())
            if not affinity:
                raise ValueError('empty CPU affinity')
            cpus.append(affinity)
            notes.append(f'CPU affinity={affinity}')
        except (OSError, ValueError, AttributeError):
            notes.append('CPU affinity=unknown')
            count = os.cpu_count()
            if count:
                cpus.append(count)
                notes.append(f'CPU count={count}')
        try:
            directories = linux_cgroup_dirs(proc, cgroup_root)
        except (OSError, ValueError, StopIteration):
            directories = []
            notes.append('cgroup v2 CPU limits=unknown')
        for directory in directories:
            label = '/' if directory == cgroup_root else '/' + directory.relative_to(cgroup_root).as_posix()
            try:
                quota, period = (directory / 'cpu.max').read_text().split()
                period = int(period)
                if period <= 0:
                    raise ValueError('invalid CPU period')
                if quota == 'max':
                    notes.append(f'cgroup {label} cpu.max=max')
                else:
                    quota = int(quota)
                    if quota <= 0:
                        raise ValueError('invalid CPU quota')
                    cpus.append(quota / period)
                    notes.append(f'cgroup {label} CPU quota={quota}/{period}')
            except (OSError, ValueError):
                notes.append(f'cgroup {label} CPU quota=unknown')
        try:
            directories = linux_cgroup_dirs(proc, cgroup_root, controller='cpu')
        except (OSError, ValueError, IndexError):
            notes.append('cgroup v1 cpu limits=unknown')
            directories = []
        for directory in directories:
            relative = directory.relative_to(directories[-1])
            label = 'cgroup v1 /' + (str(relative) if relative.parts else '')
            _cgroup_v1_cpu(directory, label, notes, cpus)
    if not cpus:
        count = os.cpu_count()
        notes.append(f'CPU count={count if count is not None else "unknown; fallback 1"}')
        cpus.append(count or 1)
    return dict(cpus=max(1, math.floor(min(cpus))),
                cargo_cpus=max(1, math.ceil(min(cpus))), notes=notes)


def process_group_live(group: int, *, system: str | None = None,
                       proc: Path = Path('/proc'), ps_output: str | None = None) -> bool | None:
    """Whether a POSIX group has an executing member, excluding zombies.

    Linux reads stat's state and process-group fields. Darwin has no stdlib
    process enumeration, so ps supplies the same two facts. Tests inject the
    platform and proc tree or ps output; parsing is independent of the host.
    Unknown observations stay unknown rather than claiming termination.
    """
    system = sys.platform if system is None else system
    try:
        if system.startswith('linux'):
            for entry in proc.iterdir():
                if not entry.name.isdigit():
                    continue
                try:
                    fields = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
                except FileNotFoundError:
                    continue
                if int(fields[2]) == group and fields[0] not in ('Z', 'X'):
                    return True
            return False
        if system == 'darwin':
            if ps_output is None:
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
