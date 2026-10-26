"""The POSIX <sys/mman.h> calls cold-page tooling shares on Linux and macOS.

Python's os module has no mincore, and its mmap module exposes no mapping
address, so mmap, munmap and mincore go through ctypes. ctypes.CDLL(None)
names the process's own C library, which resolves all three on glibc and on
libSystem alike; the protection and flag values below are the same on both.
The library is loaded on first use, never at import. Every failure raises
OSError with the call's errno; no value is substituted for a failed call.
"""
import ctypes
import mmap
import os

# mmap.PAGESIZE is defined on every supported host, unlike os.sysconf.
PAGE_SIZE = mmap.PAGESIZE
PROT_NONE = 0x0
PROT_READ = 0x1
MAP_SHARED = 0x1

_libc = None


def _system():
    """Return the process's C library with mmap, munmap and mincore declared, loading it once."""
    global _libc
    if _libc is None:
        libc = ctypes.CDLL(None, use_errno=True)
        libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int,
                              ctypes.c_int, ctypes.c_int, ctypes.c_long]
        libc.mmap.restype = ctypes.c_void_p
        libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        libc.munmap.restype = ctypes.c_int
        libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
        libc.mincore.restype = ctypes.c_int
        _libc = libc
    return _libc


def _fail(call):
    """Raise OSError for a failed call with the errno ctypes saved."""
    number = ctypes.get_errno()
    raise OSError(number, f'{call} failed: {os.strerror(number)}')


def map_file(fd, size, protection, flags=MAP_SHARED):
    """Map SIZE bytes of FD from offset zero and return the address.

    The mapping shares the file object, so a later msync can invalidate the
    cached pages rather than only this process's view.
    """
    address = _system().mmap(None, size, protection, flags, fd, 0)
    if address == ctypes.c_void_p(-1).value:
        _fail('mmap')
    return address


def unmap(address, size):
    """Remove a mapping."""
    if _system().munmap(address, size):
        _fail('munmap')


def residency(fd):
    """Return total and resident file pages from mincore.

    Map the file PROT_NONE and MAP_SHARED: mincore may examine the file's
    cached pages, and the mapping itself never faults a page in, so the
    observation does not make itself stale. An empty file has zero pages and
    is not mapped. Since Linux 5.2 mincore reports every page of a file the
    caller cannot write as resident, so a permission gap reads as resident,
    never as evicted.
    """
    size = os.fstat(fd).st_size
    if not size:
        return dict(pages=0, resident=0)
    pages = (size + PAGE_SIZE - 1) // PAGE_SIZE
    address = map_file(fd, size, PROT_NONE, MAP_SHARED)
    try:
        vector = (ctypes.c_ubyte * pages)()
        if _system().mincore(address, size, vector):
            _fail('mincore')
        return dict(pages=pages, resident=sum(value & 1 for value in vector))
    finally:
        unmap(address, size)
