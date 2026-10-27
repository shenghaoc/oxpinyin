"""Native worker kit, with the loader guard and driver cell explicitly supplied."""
import contextlib
import ctypes as C
import os
import tempfile
import time

from contract_abi import P, S, U


class Kit:
    """Hold a worker's native libraries, fresh profile and lazy facade handles.

    Capture GLib diagnostics through process-wide handlers. Context and
    instance handles are cached on demand; the kit does not automatically
    free them, remove its profile or unregister the handlers."""

    def __init__(self, mode, so, data, scratch, *, cell, check_tkrzw):
        """Load the facade and GLib, install log handlers and create a user directory.

        Check the selected tkrzw runtime before loading the facade. Register
        handlers for both facade domains, GLib's own domain and the default domain, retaining
        the callback on this object. Create the profile under scratch; native
        context initialization is deferred until requested."""
        self.mode, self.data, self.scratch = mode, data, scratch
        check_tkrzw(os.environ.get("CONTRACT_DIFF_CELL", cell), "contract-diff.py", os.environ.get("OXPINYIN_RUNTIME_SIDE", "subject"), so)
        self.lib = C.CDLL(so, use_errno=True)
        self.glib = C.CDLL('libglib-2.0.so.0')
        self.glib.g_free.argtypes = [P]
        self.glib.g_free.restype = None
        self.logs = []
        self.log_messages = []
        cb_type = C.CFUNCTYPE(None, S, U, S, P)

        def handler(domain, level, message, _unused):
            """Append the domain, level and message to the kit's log lists.

            Decode nonempty strings as UTF-8 strictly; record None for NULL
            or empty domain and message values."""
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
        self.init_seconds = 0.0  # the facade `init` calls: fixture, not case work

    def fn(self, name, res, *args):
        """Set a facade symbol's ctypes return and argument types, then return it.

        Prefix name with the kit's mode; missing symbols raise AttributeError.
        This changes the loaded function's signature without calling it."""
        f = getattr(self.lib, self.mode + '_' + name)
        f.restype, f.argtypes = res, list(args)
        return f

    def per_call_step(self):
        """Return a no-op context manager for per-call instrumentation hooks."""
        return contextlib.nullcontext()

    def init(self, system=None, user=None, *, literal=False):
        """Call the facade initializer and accumulate elapsed monotonic seconds.

        Replace false system or user values with the kit defaults unless
        literal is true. Pass None and bytes through; encode strings as UTF-8.
        Return the native pointer, possibly NULL, without caching or asserting
        success. Accumulate time even if argument conversion or the call raises."""
        def raw(value):
            """Preserve None and byte strings; encode other values with their encode method."""
            return value if value is None or isinstance(value, bytes) else value.encode()
        if not literal:
            system, user = system or self.data, user or self.user
        started = time.monotonic()
        try:
            return self.fn('init', P, S, S)(raw(system), raw(user))
        finally:
            self.init_seconds += time.monotonic() - started

    @property
    def ctx(self):
        """Initialize and cache the native context on first access, asserting success.

        A NULL result raises AssertionError and leaves initialization eligible
        for another attempt on the next access."""
        if self._ctx is None:
            self._ctx = self.init()
            assert self._ctx, 'init failed'
        return self._ctx

    def alloc(self):
        """Allocate a fresh instance from the cached context and assert it is non-NULL.

        Return the pointer without replacing the kit's cached instance."""
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
        """Decode and free a caller-owned GLib string, returning None for NULL.

        Replace invalid UTF-8 bytes during decoding and release the non-NULL
        pointer with g_free."""
        if not ptr:
            return None
        value = C.string_at(ptr).decode('utf-8', 'replace')
        self.glib.g_free(ptr)
        return value
