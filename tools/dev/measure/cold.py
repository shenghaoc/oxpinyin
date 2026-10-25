"""Per-file cold preparation for native startup measurement on APFS (§7.8).

For each measured library and data file, map the file read-only and MAP_SHARED,
touch one byte per page so the file's cached pages belong to the mapping, and
call msync(MS_SYNC | MS_INVALIDATE). Then check the whole file set with
mincore over PROT_NONE mappings. Preparation and checking happen before the
worker is spawned, outside the timed region.

A page still resident after the first whole-set check triggers one more full
preparation and check. A sample whose pages remain resident is rejected.
Darwin's metric is darwin_resident_pages; zero qualifies. Linux keeps its own
implementation (#1162) and shares REJECTION_LIMIT from this module.

Qualification results are recorded in #1243.
"""
import argparse
import contextlib
import ctypes
import json
import os
from pathlib import Path
import stat
import sys

from .. import darwin

# Shared with #1162 (Linux). Unset until a maintainer approves a value; never
# derive or tune it from campaign output. Cold mode is refused while it is None.
REJECTION_LIMIT = None

# Darwin's own name for the whole-set residency observation. Never report it
# under a Linux cold-page field.
METRIC = 'darwin_resident_pages'
ATTEMPTS = 2


class Rejected(ValueError):
    """Indicate a file set that stayed resident after the one allowed retry."""


def collect(paths):
    """Return resolved unique regular files from files and directories.

    Follow each supplied path and resolve symlinks, include every regular file
    under a supplied directory, and keep the first path for each inode. Raise
    ValueError for a missing input or a file that is not a regular file.
    """
    result, seen = [], set()
    for supplied in paths:
        root = Path(supplied).resolve(strict=True)
        candidates = [root] if root.is_file() else sorted(root.rglob('*'))
        for candidate in candidates:
            if not candidate.is_file():
                continue
            path = candidate.resolve(strict=True)
            info = path.stat()
            if not stat.S_ISREG(info.st_mode):
                raise ValueError('cold input is not a regular file: ' + str(path))
            inode = (info.st_dev, info.st_ino)
            if inode in seen:
                continue
            seen.add(inode)
            result.append(path)
    if not result:
        raise ValueError('cold preparation needs at least one regular file')
    return result


def sample_files(library, data, user):
    """Return the facade, system-data and private user-copy files for a sample."""
    return collect([library, data, user])


def _touch(address, size):
    """Read one byte from each host page so the file's pages enter the mapping."""
    for offset in range(0, size, darwin.PAGE_SIZE):
        ctypes.c_ubyte.from_address(address + offset).value


def _invalidate(fd):
    """Touch a read-only MAP_SHARED mapping, then msync(MS_SYNC | MS_INVALIDATE)."""
    size = os.fstat(fd).st_size
    if not size:
        return
    address = darwin.map_file(fd, size, darwin.PROT_READ, darwin.MAP_SHARED)
    try:
        _touch(address, size)
        darwin.msync(address, size, darwin.MS_SYNC | darwin.MS_INVALIDATE)
    finally:
        darwin.unmap(address, size)


def _check(handle, path):
    """Return one file's mincore residency, wrapping syscall failures as ValueError."""
    try:
        return dict(path=str(path), **darwin.file_residency(handle.fileno()))
    except OSError as error:
        raise ValueError(f'cold residency check failed: {path}: {error}') from error


def prepare(files):
    """Prepare a file set cold, check the whole set, and allow one retry.

    Keep one descriptor per inode open so every preparation and every check
    names the same file. Flush each file first: a freshly copied user profile
    can still have dirty pages that MS_INVALIDATE would not drop. Return a
    record keyed by METRIC when the whole set is non-resident, or raise
    Rejected after ATTEMPTS preparations leave a page resident. File bytes are
    not read after preparation; the check uses PROT_NONE mappings only.
    """
    paths = [Path(path) for path in files]
    if not paths:
        raise ValueError('cold preparation needs at least one file')
    with contextlib.ExitStack() as stack:
        opened = []
        for path in paths:
            try:
                handle = stack.enter_context(path.open('rb'))
                info = os.fstat(handle.fileno())
                if not stat.S_ISREG(info.st_mode):
                    raise ValueError('cold input is not a regular file: ' + str(path))
                os.fsync(handle.fileno())
            except OSError as error:
                raise ValueError(f'cold preparation failed: {path}: {error}') from error
            opened.append((path, handle))
        checks = []
        for attempt in range(1, ATTEMPTS + 1):
            for path, handle in opened:
                try:
                    _invalidate(handle.fileno())
                except OSError as error:
                    raise ValueError(f'cold preparation failed: {path}: {error}') from error
            # Check every file only after all of them were prepared, so a later
            # preparation cannot hide an earlier file's refault.
            rows = [_check(handle, path) for path, handle in opened]
            resident = sum(row['resident'] for row in rows)
            pages = sum(row['pages'] for row in rows)
            checks.append(dict(attempt=attempt, pages=pages, resident=resident))
            if resident == 0:
                return dict(metric=METRIC, value=0, pages=pages, resident=0,
                            attempts=attempt, verdict='QUALIFIED', checks=checks, files=rows)
    details = '; '.join(f"{row['path']} ({row['resident']}/{row['pages']})"
                        for row in rows if row['resident'])
    raise Rejected(f'cold sample rejected: {resident} of {pages} {METRIC} still resident '
                   f'after {ATTEMPTS} preparations: {details}')


def demonstrate(files):
    """Record the qualification control before whole-set preparation.

    For each file, touch every page, observe residency, call msync(MS_SYNC)
    alone, observe again, then call msync(MS_SYNC | MS_INVALIDATE) and observe
    a third time. The first observation must equal all pages, the second must
    stay resident, and the third must be zero; otherwise the host does not
    support the preparation this module relies on.
    """
    rows = []
    with contextlib.ExitStack() as stack:
        for path in files:
            handle = stack.enter_context(Path(path).open('rb'))
            fd = handle.fileno()
            size = os.fstat(fd).st_size
            if not size:
                rows.append(dict(path=str(path), pages=0, before=0, msync_sync=0, after=0))
                continue
            os.fsync(fd)
            address = darwin.map_file(fd, size, darwin.PROT_READ, darwin.MAP_SHARED)
            try:
                _touch(address, size)
                before = darwin.file_residency(fd)
                darwin.msync(address, size, darwin.MS_SYNC)
                synced = darwin.file_residency(fd)
                darwin.msync(address, size, darwin.MS_SYNC | darwin.MS_INVALIDATE)
                after = darwin.file_residency(fd)
            finally:
                darwin.unmap(address, size)
            rows.append(dict(path=str(path), pages=before['pages'],
                             before=before['resident'], msync_sync=synced['resident'],
                             after=after['resident']))
    qualified = all(row['before'] == row['pages'] and row['msync_sync'] == row['pages'] and
                    row['after'] == 0 for row in rows)
    return dict(metric=METRIC, verdict='QUALIFIED' if qualified else 'REJECTED',
                rule='before == pages, MS_SYNC == pages, MS_SYNC|MS_INVALIDATE == 0',
                files=rows)


def refusal():
    """Return the reason cold mode is refused, or None once a limit is approved."""
    if REJECTION_LIMIT is None:
        return 'cold cache requires an approved rejection limit (§7.8); none is set'
    return None


def main(argv=None):
    """Qualify APFS cold preparation on named files and print one JSON record.

    This is a read-only qualification probe, not a measurement. It refuses
    non-macOS hosts, prints the per-file control and the whole-set preparation
    record, and returns 1 when the control or preparation fails.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('paths', nargs='+', type=Path,
                        help='files or directories on one APFS volume')
    args = parser.parse_args(argv)
    if sys.platform != 'darwin':
        print('cold-qualify: native macOS only', file=sys.stderr)
        return 1
    try:
        files = collect(args.paths)
        control = demonstrate(files)
        if control['verdict'] != 'QUALIFIED':
            raise Rejected('cold qualification control failed: ' + json.dumps(control, sort_keys=True))
        prepared = prepare(files)
    except (OSError, ValueError) as error:
        print('cold-qualify: ' + str(error), file=sys.stderr)
        return 1
    print(json.dumps(dict(qualification=control, preparation=prepared), sort_keys=True))
    return 0
