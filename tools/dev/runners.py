"""Registry runner services and explicit recipe dispatch, sharing pool child cleanup.

The registry remains in run-all.py/runners.json. This module supplies the
build/cell/data/driver operations used by its recipes, not another scheduler.
"""
from collections.abc import Callable, Mapping, Sequence
from contextlib import ExitStack
from dataclasses import dataclass, field, replace
import io
import importlib.util
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

from . import host, pool, slot
from .cell import CellError, ROOT, Settings, library_path
from .drivers import DriverServices
from .system_data import SystemData


class Finished(Exception):
    def __init__(self, status: int):
        self.status = status


def emit(value: str = '', *, error: bool = False) -> None:
    print(value, file=sys.stderr if error else sys.stdout, flush=True)


def write(data: bytes, *, error: bool = False) -> None:
    stream = sys.stderr if error else sys.stdout
    stream.flush()
    stream.buffer.write(data)
    stream.flush()


def status(code: int) -> int:
    return 128 - code if code < 0 else code


@dataclass
class Context:
    settings: Settings
    services: DriverServices
    stack: ExitStack
    caller: Path
    name: str
    events: list[tuple[bool, bytes]] | None = None
    retained: set[Path] = field(default_factory=set)

    def emit(self, value: str = '', *, error: bool = False) -> None:
        self.write((value + '\n').encode(), error=error)

    def write(self, data: bytes, *, error: bool = False) -> None:
        if self.events is None:
            write(data, error=error)
        else:
            self.events.append((error, data))

    def campaign(self, items: Sequence, function: Callable) -> list:
        """Run independent cases with private scratch and deterministic streams.

        Admission and child cleanup are pool.py's, including inherited slots.
        Dependent steps within each case remain sequential in its callback.
        """
        def unit(item):
            def run(context):
                events = []
                if not context.fds and self.services.context is not None:
                    context = replace(context, fds=self.services.context.fds)
                environment = dict(context.environment, **self.env)
                for key in ('OXPINYIN_HEAVY_SLOT_HELD', 'OXPINYIN_LOCK_DIR', pool.POOL_WORKERS):
                    if key in context.environment:
                        environment[key] = context.environment[key]
                settings = replace(self.settings, environment=environment)
                with ExitStack() as stack:
                    services = self.services.fork(settings, context,
                        stdout=lambda data: events.append((False, data)),
                        stderr=lambda data: events.append((True, data)))
                    child = replace(self, settings=settings, services=services, stack=stack, events=events)
                    try:
                        return function(child, item), events
                    except (Finished, CellError) as error:
                        error.events = events
                        raise
            return run

        def emit_unit(name, result):
            for error, data in result[1]:
                self.write(data, error=error)

        try:
            results = pool.run_admitted([(f'{index}:{item}', unit(item)) for index, item in enumerate(items)],
                                        self.name, cancelled=slot.CANCELLED.is_set, emit=emit_unit,
                                        out=io.StringIO())
        except (Finished, CellError) as error:
            for diagnostic, data in getattr(error, 'events', []):
                self.write(data, error=diagnostic)
            raise
        return [result[0] for result in results]

    @property
    def env(self) -> Mapping[str, str]:
        return self.settings.environment

    @property
    def here(self) -> Path:
        return self.settings.root / 'tools/bisection'

    @property
    def data(self) -> SystemData:
        return SystemData(self.settings, self.here)

    def scratch(self) -> Path:
        directory = Path(tempfile.mkdtemp())
        self.stack.callback(lambda: None if directory in self.retained else shutil.rmtree(directory))
        return directory

    def output(self, variable: str) -> Path:
        value = self.env.get(variable)
        if not value:
            return self.scratch()
        directory = self.path(value)
        directory.mkdir(parents=True, exist_ok=True)
        return directory

    def path(self, value: str | Path, *, caller: bool = False) -> Path:
        path = Path(value)
        return path if path.is_absolute() else (self.caller if caller else self.here) / path

    def command(self, argv: Sequence[str | Path], *, cwd: Path | None = None,
                environment: Mapping[str, str] | None = None, capture: bool = False):
        return self.services.command(argv, cwd=cwd or self.here, environment=environment, capture=capture)

    def checked(self, argv: Sequence[str | Path], **options):
        result = self.command(argv, **options)
        if result.returncode:
            raise Finished(status(result.returncode))
        return result

    def flags(self, *args: str, optional: bool = False) -> list[str]:
        result = self.command(['pkg-config', *args, 'glib-2.0'], capture=True)
        if result.returncode and not optional:
            self.write(result.stderr, error=True)
            raise Finished(status(result.returncode))
        return shlex.split(result.stdout.decode()) if result.returncode == 0 else []

    def compile(self, stem: str, destination: Path | None = None, *, compiler: str = 'gcc',
                flags: Sequence[str] = ('-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2'),
                glib: bool = False, optional_glib: bool = False, suffix: str = '.c',
                link_flags: Sequence[str] = (), absolute_source: bool = False) -> Path:
        destination = destination or self.here / stem
        extra = self.flags('--cflags', '--libs', optional=optional_glib) if glib or optional_glib else []
        source = self.here / (stem + suffix) if absolute_source else Path(stem + suffix)
        self.checked([compiler, *flags, '-o', destination, source, '-ldl', *extra, *link_flags])
        return destination

    def artifact(self, mode: str = 'pinyin') -> Path:
        variable, filename, package = ('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi') if mode == 'pinyin' else (
            'OXPINYIN_ZHUYIN_SO', 'libzhuyin_capi.so', 'oxpinyin-zhuyin-capi')
        return self.settings.artifact(variable, filename, package)

    def pin(self, *, missing: int = 77, hint: bool = False, exact: bool = True,
            require_data: bool = False, verify: bool = True) -> tuple[Path, Path]:
        prefix = self.path(self.env.get('PINYIN_ORACLE_PREFIX') or str(Path.home() / '.local/opt/pinyin-oracle'))
        so, data = prefix / 'lib/libpinyin.so', prefix / 'lib/libpinyin/data'
        if not (prefix / 'oracle-pin.txt').is_file() or not so.is_file():
            emit(f'SKIP: pin-built oracle not found at {prefix}')
            if hint:
                emit('  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX')
            raise Finished(missing)
        lines = (prefix / 'oracle-pin.txt').read_text().splitlines()
        good = self.settings.expected_pin in lines if exact else any(
            line.startswith(self.settings.expected_pin.split('+model20-')[0]) for line in lines)
        if verify and not good:
            emit(f'SKIP: oracle prefix at {prefix} is off-pin' + (f' for the {self.settings.cell} cell' if exact else ''))
            if exact:
                emit('  expected ' + self.settings.expected_pin)
            raise Finished(missing)
        if require_data and not (data / 'bigram.db').is_file():
            emit(f'SKIP: oracle data not found at {data}')
            raise Finished(missing)
        return so, data

    def run(self, driver: Path, so: Path, args: Sequence[str | Path], *, side: str,
            cwd: Path | None = None, env: Mapping[str, str] | None = None,
            loader: bool = False, quiet: bool = False, allow_failure: bool = True,
            umask: int = -1):
        environment = dict(self.env, **(env or {}))
        if loader:
            inherited = library_path(self.env.get('LD_LIBRARY_PATH', ''), self.caller)
            environment['LD_LIBRARY_PATH'] = str(so.parent) + (':' + inherited if inherited else '')
        result = self.services.run([str(driver), *map(str, args)], so, runner=self.name, side=side,
                                   cwd=cwd or self.here, environment=environment, umask=umask)
        if quiet:
            result.stderr = b''
        if not allow_failure and result.returncode:
            self.write(result.stderr, error=True)
            raise Finished(status(result.returncode))
        return result

    def compare(self, pin: bytes, subject: bytes, *, labels: tuple[str, str] | None = None,
                paths: tuple[Path, Path] | None = None) -> tuple[int, bytes]:
        if paths is None:
            work = self.scratch()
            paths = work / 'oracle.log', work / 'oxpinyin.log'
            paths[0].write_bytes(pin)
            paths[1].write_bytes(subject)
        command = ['diff', '-u']
        if labels:
            command += ['--label', labels[0], '--label', labels[1]]
        result = self.command([*command, *paths], capture=True)
        if result.stderr:
            write(result.stderr, error=True)
        return result.returncode, result.stdout

    def pair(self, driver: Path, pin: Path, subject: Path, pin_data: Path, subject_data: Path,
             *, args: Sequence[str] = (), loader: bool = False, fresh: bool = False,
             quiet: bool = False, pin_first: bool = False, env: Mapping[str, str] | None = None,
             failure: tuple[str, str] | None = None, diagnostics: bool = False) -> tuple[bytes, bytes]:
        observations = {}
        order = ('pin', 'subject') if pin_first else ('subject', 'pin')
        for side in order:
            so, data = (pin, pin_data) if side == 'pin' else (subject, subject_data)
            result = self.run(driver, so, [so, data, *args], side=side, loader=loader, quiet=quiet,
                              cwd=self.scratch() if fresh else None, env=env)
            if result.returncode:
                label = ('the oracle' if side == 'pin' else 'oxpinyin-capi')
                self.emit(failure[0 if side == 'subject' else 1] if failure else f'FAIL: {driver.name} crashed against {label}')
                self.write(result.stdout)
                if diagnostics:
                    self.emit('--- driver diagnostics (stderr) ---')
                    self.write(result.stderr)
                raise Finished(1)
            if not quiet and not diagnostics:
                self.write(result.stderr, error=True)
            observations[side] = result.stdout
        return observations['pin'], observations['subject']


def python_script(ctx: Context, filename: str, argv: Sequence[str]) -> int:
    return status(ctx.command([sys.executable, '-X', 'lazy_imports=normal', ctx.here / filename, *argv]).returncode)


def main(argv: Sequence[str]) -> int:
    if not argv:
        emit('usage: runner RECIPE [ARG ...]', error=True)
        return 2
    for name in ('UV_PYTHON', 'PYTHONHOME', 'PYTHONPATH'):
        if os.environ.get(name):
            emit(f'FAIL: {name} is not supported; unset it before running repository Python', error=True)
            return 2
    from .runner_recipes import RECIPES
    recipe, *args = argv
    if recipe == 'run-all':
        command = [sys.executable, '-X', 'lazy_imports=normal', str(ROOT / 'tools/bisection/run-all.py'), *args]
        os.execv(sys.executable, command)
    if recipe == 'contract-diff':
        if args in (['-h'], ['--help']):
            os.execv(sys.executable, [sys.executable, '-X', 'lazy_imports=normal',
                                     str(ROOT / 'tools/bisection/contract-diff.py'), *args])
        if len(args) < 3:
            emit('expected backend, oracle prefix and subject libpinyin', error=True)
            return 1
        cell, prefix, pinyin, *tail = args
        if cell not in ('bdb', 'kc', 'tkrzw'):
            emit(f'unknown backend: {cell}', error=True)
            return 1
        forwarded = [cell, prefix, pinyin]
        if tail and not tail[0].startswith('-'):
            forwarded += ['--zhuyin-so', tail.pop(0)]
        if tail[:1] == ['--']:
            tail = tail[1:]
        os.execv(sys.executable, [sys.executable, '-X', 'lazy_imports=normal',
                                str(ROOT / 'tools/bisection/contract-diff.py'), *forwarded, *tail])
    if recipe not in RECIPES:
        emit(f'unknown runner recipe: {recipe}', error=True)
        return 2
    def call() -> int:
        with ExitStack() as stack:
            settings = Settings.read(os.environ)
            context = pool.UnitContext(recipe, slot.CANCELLED, environment=dict(settings.environment),
                                       fds=slot.inherited_fds(recipe, out=io.StringIO()))
            ctx = Context(settings, DriverServices(settings, context), stack, Path.cwd(), recipe)
            return RECIPES[recipe](ctx, args)
    return guarded(call)


def guarded(call: Callable[[], int]) -> int:
    """Run CALL under the cancellation handlers and return its status.

    The failures every runner shares end in the same statuses and messages: a
    finished or refused run, a cancellation, a blocked admission, a missing
    command (127), and any other OS or value error (1).
    """
    try:
        with slot.cancellation_handlers():
            return call()
    except Finished as error:
        return error.status
    except CellError as error:
        emit(str(error), error=True)
        return error.status
    except (pool.UnitStopped, pool.UnitsCancelled, slot.Cancelled) as error:
        emit(str(error), error=True)
        return slot.cancellation_status()
    except host.ResourceBlocked as error:
        emit(str(error), error=True)
        return 1
    except FileNotFoundError as error:
        emit(str(error), error=True)
        return 127
    except (OSError, ValueError, RuntimeError) as error:
        emit(f'FAIL: {error}', error=True)
        return 1
