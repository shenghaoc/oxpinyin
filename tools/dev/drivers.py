"""Shared compiler and C-driver execution, private fixtures and comparisons."""
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass, replace
from pathlib import Path
import shutil
import subprocess
import sys

from .cell import CellError, Settings
from .pool import UnitContext
from tools.tkrzw.runtime import RuntimeRefused, check, launched


@dataclass(frozen=True)
class DriverServices:
    settings: Settings
    context: UnitContext | None = None
    stdout: Callable[[bytes], None] | None = None
    stderr: Callable[[bytes], None] | None = None

    def fork(self, settings: Settings, context: UnitContext,
             stdout: Callable[[bytes], None], stderr: Callable[[bytes], None]) -> 'DriverServices':
        return replace(self, settings=settings, context=context, stdout=stdout, stderr=stderr)

    def command(self, argv: Sequence[str], *, cwd: Path | None = None,
                environment: Mapping[str, str] | None = None, capture: bool = False,
                merged: bool = False, umask: int = -1) -> subprocess.CompletedProcess:
        """Launch one dependent compilation or driver step with explicit context."""
        # Preserve parent/child stream order when the CLI itself is redirected.
        sys.stdout.flush()
        sys.stderr.flush()
        if self.context is not None:
            result = self.context.launch(list(map(str, argv)), cwd=cwd or self.settings.root,
                                         env=self.settings.environment if environment is None else environment,
                                         merge_stderr=merged, umask=umask)
            if not capture:
                if self.stdout is None:
                    sys.stdout.buffer.write(result.stdout)
                else:
                    self.stdout(result.stdout)
                if self.stderr is None:
                    sys.stderr.buffer.write(result.stderr)
                else:
                    self.stderr(result.stderr)
                sys.stdout.flush()
                sys.stderr.flush()
            return subprocess.CompletedProcess(argv, result.status,
                                               result.stdout if capture else None,
                                               result.stderr if capture else None)
        return subprocess.run(list(map(str, argv)), cwd=cwd or self.settings.root,
                              env=self.settings.environment if environment is None else environment,
                              umask=umask,
                              stdout=subprocess.PIPE if capture else None,
                              stderr=subprocess.STDOUT if merged else (subprocess.PIPE if capture else None))

    def run(self, argv: Sequence[str], obj: Path, *, runner: str, side: str,
            cwd: Path, environment: Mapping[str, str] | None = None,
            capture: bool = True, merged: bool = False, umask: int = -1) -> subprocess.CompletedProcess:
        environment = self.settings.environment if environment is None else environment
        # U3 owns the runtime resolver. Reuse its existing adapter throughout
        # this migration; do not introduce another ELF inspection path.
        self.check_object(obj, runner=runner, side=side, environment=environment, cwd=cwd)
        executable = Path(argv[0])
        if not executable.is_absolute():
            executable = ((cwd / executable) if '/' in str(executable) else
                          Path(shutil.which(str(executable), path=environment.get('PATH')) or executable))
        started = launched(executable, cwd)
        if started is not None and started != obj:
            self.check_object(started, runner=runner, side=side, environment=environment, cwd=cwd)
        return self.command(argv, cwd=cwd, environment=environment, capture=capture, merged=merged, umask=umask)

    def check_object(self, obj: Path, *, runner: str, side: str, cwd: Path,
                     environment: Mapping[str, str]) -> None:
        """Check another dlopened facade through the same runtime adapter."""
        try:
            check(self.settings.cell, runner, side, obj, env=environment, cwd=cwd)
        except RuntimeRefused as error:
            raise CellError('REFUSED: ' + str(error)) from error

    def mini_fixture(self, destination: Path) -> None:
        source = self.settings.root / 'fixtures/w3' / self.settings.extension
        destination.mkdir(parents=True)
        for file in source.iterdir():
            if file.is_file():
                shutil.copyfile(file, destination / file.name)
        (destination / 'interpolation2.text').write_text(
            '\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n')
