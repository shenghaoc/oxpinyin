"""Native worker kit: a facade handle, GLib diagnostics and one user profile."""
import ctypes as C
import os
import tempfile

from contract_abi import P, S, U
from tools.tkrzw.runtime import check


class Kit:
    """Hold a worker's native libraries, fresh profile and lazy facade handles."""

    def __init__(self, mode, so, data, scratch, *, cell):
        """Load the facade and GLib, install log handlers and create a user directory.

        Check the selected tkrzw runtime before loading the facade. Register
        handlers for both facade domains, GLib's own domain and the default
        domain. The profile lives under scratch; initialization is deferred."""
        self.mode, self.data = mode, data
        check(cell, 'reverse_cross_open.py',
              os.environ.get('OXPINYIN_RUNTIME_SIDE', 'subject'), so)
        self.lib = C.CDLL(so, use_errno=True)
        # GLib's functions resolve through the facade's handle: dlsym searches the
        # library and the libraries it depends on.
        self.glib = C.CDLL(so)
        self.logs = []
        cb_type = C.CFUNCTYPE(None, S, U, S, P)

        def handler(domain, level, _message, _unused):
            self.logs.append([domain.decode() if domain else None, level])
        self._handler = cb_type(handler)
        self.glib.g_log_set_handler.argtypes = [S, U, cb_type, P]
        self.glib.g_log_set_handler.restype = U
        for domain in (b'libpinyin', b'libzhuyin', b'GLib', None):
            self.glib.g_log_set_handler(domain, 0xFFFFFFFC, self._handler, None)
        self._ctx = None
        self.user = tempfile.mkdtemp(prefix='user-', dir=scratch)

    def fn(self, name, res, *args):
        """Set a facade symbol's ctypes return and argument types, then return it."""
        f = getattr(self.lib, self.mode + '_' + name)
        f.restype, f.argtypes = res, list(args)
        return f

    def init(self):
        """Open this worker's data directory and user profile."""
        return self.fn('init', P, S, S)(os.fspath(self.data).encode(), os.fspath(self.user).encode())

    @property
    def ctx(self):
        """Initialize and cache the native context on first access, asserting success."""
        if self._ctx is None:
            self._ctx = self.init()
            assert self._ctx, 'init failed'
        return self._ctx
