"""Per-file cold preparation for native startup measurement on Linux ext4 (§7.8).

The Linux sibling of tools.dev.measure.cold, with the same record shape. Before
each cold sample, fsync every regular file in the sample's private user copy,
then evict the copied facade, every system-data file and every user-copy file
with posix_fadvise(POSIX_FADV_DONTNEED), and check the whole set with mincore
over PROT_NONE mappings through tools.dev.mman, the binding macOS shares.
Preparation and checking happen before the worker is spawned, outside the
timed region.

A page still resident after the first whole-set check triggers one more full
eviction and check. A sample whose pages remain resident is rejected exactly
as on macOS: cold.Rejected, counted against cold.REJECTION_LIMIT, which this
module shares and never redefines. Linux's metric is linux_resident_pages;
zero qualifies. It is never written under the Darwin field.
"""
import contextlib
import os
from pathlib import Path
import stat

from .. import mman
from . import cold, native

# Linux's own name for the whole-set residency observation. Never report it
# under the Darwin cold-page field.
METRIC = 'linux_resident_pages'
ATTEMPTS = cold.ATTEMPTS
Rejected = cold.Rejected

def evict(fd):
    """Drop one file's clean cached pages with posix_fadvise(POSIX_FADV_DONTNEED)."""
    os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)


def _check(handle, path):
    """Return one file's mincore residency, wrapping syscall failures as ValueError."""
    try:
        return dict(path=str(path), **mman.residency(handle.fileno()))
    except OSError as error:
        raise ValueError(f'cold residency check failed: {path}: {error}') from error


def prepare(files, user):
    """Evict a sample's file set, check the whole set, and allow one retry.

    FILES is the sample's resolved set (cold.sample_files); every file in it
    under USER, the private user copy, is flushed with fsync before the first
    eviction, because the copy was just written and DONTNEED leaves dirty
    pages. Keep one descriptor per file open so every eviction and every check
    names the same inode. Refuse a file that is not on ext4. Return a record
    keyed by METRIC when the whole set is non-resident, or raise Rejected
    after ATTEMPTS evictions leave a page resident; the exception carries the
    same record with verdict REJECTED and the resident page count as its
    value. File bytes are never read.
    """
    paths = [Path(path) for path in files]
    if not paths:
        raise ValueError('cold preparation needs at least one file')
    user = Path(user).resolve(strict=True)
    with contextlib.ExitStack() as stack:
        opened = []
        for path in paths:
            try:
                kind, _ = native.filesystem(path)
                if kind != 'ext4':
                    raise ValueError(f'cold cache requires ext4: {path} is on {kind}')
                handle = stack.enter_context(path.open('rb'))
                info = os.fstat(handle.fileno())
                if not stat.S_ISREG(info.st_mode):
                    raise ValueError('cold input is not a regular file: ' + str(path))
                if path.resolve().is_relative_to(user):
                    os.fsync(handle.fileno())
            except OSError as error:
                raise ValueError(f'cold preparation failed: {path}: {error}') from error
            opened.append((path, handle))
        checks = []
        for attempt in range(1, ATTEMPTS + 1):
            for path, handle in opened:
                try:
                    evict(handle.fileno())
                except OSError as error:
                    raise ValueError(f'cold preparation failed: {path}: {error}') from error
            # Check every file only after all of them were evicted, so a later
            # eviction cannot hide an earlier file's refault.
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
                   f'after {ATTEMPTS} evictions: {details}',
                   dict(metric=METRIC, value=resident, pages=pages, resident=resident,
                        attempts=ATTEMPTS, verdict='REJECTED', checks=checks, files=rows))
