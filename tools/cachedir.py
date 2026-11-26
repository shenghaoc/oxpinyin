"""The cache root of the repository's tooling: $OXPINYIN_CACHE.

Everything the tooling fetches or builds from pinned sources goes under this
one directory, which every worktree shares. The justfile exports it: a non-empty
OXPINYIN_CACHE, else `oxpinyin` in the user's cache directory. A
caller that does not start from a recipe (CI, tools.dev.provision, a
container run) sets it itself. `just cache-root` prints it.

Each entry carries its pinned version or hash in its name, so a new pin gets a
new entry: contract-data/<version>, model20-<digest>, tkrzw-<revision> and
pin-<commit>.
"""
from contextlib import contextmanager
import errno
import os
from pathlib import Path


def root(env=None):
    """The cache root ENV names (default: the process environment), which must be absolute."""
    value = (os.environ if env is None else env).get('OXPINYIN_CACHE', '')
    if not os.path.isabs(value):
        raise SystemExit(f'FAIL: OXPINYIN_CACHE must name an absolute directory, not {value!r}; '
                         'start the tool from its just recipe, or set the variable '
                         '(docs/runbooks/setup.md#cache-directory)')
    return Path(value)


@contextmanager
def exclusive_fetch(entry):
    """Allow one fetch per pinned entry. A busy entry is left untouched."""
    lock = entry.with_name(f'.{entry.name}.fetch-lock')
    lock.parent.mkdir(parents=True, exist_ok=True)
    with lock.open('a+b') as stream:
        stream.seek(0)
        fd = stream.fileno()
        try:
            if os.name == 'nt':
                import msvcrt
                msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            if error.errno not in (errno.EACCES, errno.EAGAIN):
                raise
            raise OSError(f'cache entry is busy: {lock} (see docs/runbooks/setup.md#cache-directory)') from None
        try:
            yield
        finally:
            if os.name == 'nt':
                msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(fd, fcntl.LOCK_UN)
