"""Native worker kit: a facade handle, GLib diagnostics and one user profile."""
import ctypes as C
import os
import tempfile

from contract_abi import P, S, U


class Kit:
    """Hold a worker's native libraries, fresh profile and lazy facade handles."""

    def __init__(self, mode, so, data, scratch, *, cell, check_tkrzw):
        """Load the facade and GLib, install log handlers and create a user directory.

        Check the selected tkrzw runtime before loading the facade. Register
        handlers for both facade domains, GLib's own domain and the default
        domain. The profile lives under scratch; initialization is deferred."""
        self.mode, self._data, self.scratch = mode, data, scratch
        check_tkrzw(cell, 'reverse_cross_open.py',
                    os.environ.get('OXPINYIN_RUNTIME_SIDE', 'subject'), so)
        self.lib = C.CDLL(so, use_errno=True)
        # GLib's functions resolve through the facade's handle: dlsym searches the
        # library and the libraries it depends on.
        self.glib = C.CDLL(so)
        self.glib.g_free.argtypes = [P]
        self.glib.g_free.restype = None
        self.logs = []
        self.log_messages = []
        cb_type = C.CFUNCTYPE(None, S, U, S, P)

        def handler(domain, level, message, _unused):
            self.logs.append([domain.decode() if domain else None, level])
            self.log_messages.append(message.decode() if message else None)
        self._handler = cb_type(handler)
        self.glib.g_log_set_handler.argtypes = [S, U, cb_type, P]
        self.glib.g_log_set_handler.restype = U
        for domain in (b'libpinyin', b'libzhuyin', b'GLib', None):
            self.glib.g_log_set_handler(domain, 0xFFFFFFFC, self._handler, None)
        self._ctx = None
        self._inst = None
        self.user = tempfile.mkdtemp(prefix='user-', dir=scratch)

    @property
    def data(self):
        return self._data

    @data.setter
    def data(self, value):
        self._data = value

    def fn(self, name, res, *args):
        """Set a facade symbol's ctypes return and argument types, then return it."""
        f = getattr(self.lib, self.mode + '_' + name)
        f.restype, f.argtypes = res, list(args)
        return f

    def init(self, system=None, user=None, *, literal=False):
        """Call the facade initializer and return its pointer, possibly NULL."""
        def raw(value):
            return value if value is None or isinstance(value, bytes) else os.fspath(value).encode()
        if not literal:
            system, user = system or self.data, user or self.user
        return self.fn('init', P, S, S)(raw(system), raw(user))

    @property
    def ctx(self):
        """Initialize and cache the native context on first access, asserting success."""
        if self._ctx is None:
            self._ctx = self.init()
            assert self._ctx, 'init failed'
        return self._ctx

    def alloc(self):
        """Allocate a fresh instance from the cached context and assert it is non-NULL."""
        inst = self.fn('alloc_instance', P, P)(self.ctx)
        assert inst
        return inst

    @property
    def inst(self):
        """Allocate and cache one native instance on first access."""
        if self._inst is None:
            self._inst = self.alloc()
        return self._inst

    def text(self, ptr):
        """Decode and free a caller-owned GLib string, returning None for NULL."""
        if not ptr:
            return None
        value = C.string_at(ptr).decode('utf-8', 'replace')
        self.glib.g_free(ptr)
        return value
