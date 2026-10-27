"""Shared native tool discovery, process launch and tkrzw runtime qualification.

The native lint, backend and sanitizer gates each need the same three things:
find the tools a host provides, run a child with an explicit directory and
environment, and certify the libtkrzw a binary actually loads.  This module is
the one implementation of those operations; the gates stay data and policy.
"""
from dataclasses import dataclass, field
import os
from pathlib import Path
import shutil
import subprocess
import sys

from . import slot
from tools.tkrzw import runtime

ROOT = Path(__file__).resolve().parents[2]


class NativeRefused(RuntimeError):
    """Indicate that a required host tool or runtime could not be qualified."""


def shell_status(code):
    """Return the shell-style status for a subprocess status, including signals."""
    return 128 - code if code < 0 else code


def launch_status(error):
    """Return the shell's status for a command that could not be launched.

    A command that is not found is 127; one that is found but cannot be
    executed, such as a file without execute permission, is 126.
    """
    return 127 if isinstance(error, FileNotFoundError) else 126


@dataclass
class Tools:
    """Discover and launch the host tools one native gate depends on."""

    environment: dict[str, str] = field(default_factory=lambda: dict(os.environ))
    root: Path = ROOT

    def which(self, name):
        """Return the executable NAME resolves to through this environment's PATH."""
        return shutil.which(name, path=self.environment.get('PATH'))

    def run(self, argv, *, cwd=None, environment=None, input=None, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE):
        """Launch ARGV and return its completed process.

        The caller chooses the streams: the defaults capture both as text, while
        None inherits the parent's stream and DEVNULL discards one.
        """
        settings = self.environment if environment is None else environment
        return subprocess.run([str(part) for part in argv], cwd=cwd or self.root,
                              env=dict(settings), input=input, text=text,
                              stdout=stdout, stderr=stderr)

    def admitted(self, argv, *, environment=None, out=sys.stderr):
        """Run ARGV under the shared heavy slot and memory admission."""
        settings = self.environment if environment is None else environment
        return slot.run_command([str(part) for part in argv], env=dict(settings), out=out)

    def tkrzw(self, obj, *, runner, side, cwd, environment=None):
        """Return the qualified libtkrzw path for OBJ, or None when it has none.

        Reuse the single runtime adapter; a refusal is re-raised as NativeRefused
        with the resolver's diagnostic intact.
        """
        settings = self.environment if environment is None else environment
        try:
            return runtime.check('tkrzw', runner, side, obj, env=dict(settings), cwd=cwd)
        except runtime.RuntimeRefused as error:
            raise NativeRefused(str(error)) from error

    def tkrzw_host(self, *, environment=None):
        """Return the exact host library certified by the provisioning service."""
        from .provision import probe_host
        import contextlib
        settings = self.environment if environment is None else environment
        with contextlib.redirect_stdout(sys.stderr):
            library = probe_host(env=dict(settings), root=self.root)
        if not library:
            raise NativeRefused('probe_host exited 1')
        return Path(library)
