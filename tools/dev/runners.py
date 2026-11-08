"""Registry runner services and explicit recipe dispatch, sharing pool child cleanup.

The registry remains in run-all.py/runners.json. This module supplies the
cell and driver operations used by its recipe, not another scheduler.
"""
from collections.abc import Callable, Mapping, Sequence
from contextlib import ExitStack
from dataclasses import dataclass
import os
from pathlib import Path
import shlex
import shutil
import sys
import tempfile

from . import pool, slot
from .cell import CellError, ROOT, Settings, library_path
from .drivers import DriverServices


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

    def emit(self, value: str = '', *, error: bool = False) -> None:
        self.write((value + '\n').encode(), error=error)

    def write(self, data: bytes, *, error: bool = False) -> None:
        if self.events is None:
            write(data, error=error)
        else:
            self.events.append((error, data))

    @property
    def env(self) -> Mapping[str, str]:
        return self.settings.environment

    @property
    def here(self) -> Path:
        return self.settings.root / 'tools/bisection'

    def scratch(self) -> Path:
        directory = Path(tempfile.mkdtemp())
        self.stack.callback(shutil.rmtree, directory)
        return directory

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

    def artifact(self, mode: str = 'pinyin') -> Path:
        variable, filename, package = ('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi') if mode == 'pinyin' else (
            'OXPINYIN_ZHUYIN_SO', 'libzhuyin_capi.so', 'oxpinyin-zhuyin-capi')
        return self.settings.artifact(variable, filename, package)

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

def verify_contract(args: Sequence[str]) -> int:
    """`contract-diff verify CELL PINYIN_SO [ZHUYIN_SO] [-- OPTIONS]`: the subject against the references.

    The tkrzw cell reads the pinned libpinyin-data package; it is fetched into target/contract-data
    when absent (`just fetch-contract-data`). Other options pass through to contract-diff.py."""
    cell, pinyin, *tail = args
    if cell not in ('bdb', 'kc', 'tkrzw'):
        emit(f'unknown backend: {cell}', error=True)
        return 1
    forwarded = ['verify', cell, pinyin]
    if tail and not tail[0].startswith('-'):
        forwarded += ['--zhuyin-so', tail.pop(0)]
    if tail[:1] == ['--']:
        tail = tail[1:]
    if cell == 'tkrzw' and '--data' not in tail:
        from . import contract_data
        forwarded += ['--data', str(contract_data.fetch(contract_data.DEFAULT_CACHE))]
    os.execv(sys.executable, [sys.executable, '-X', 'lazy_imports=normal',
                              str(ROOT / 'tools/bisection/contract-diff.py'), *forwarded, *tail])


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
        operation = 'verify'
        if args[:1] in (['verify'], ['capture']):
            operation, *args = args
        if operation == 'capture':
            emit('capture runs in the Debian stable pin image: bash tools/oracle/capture-contract.sh CELL [options]',
                 error=True)
            return 1
        if len(args) < 2:
            emit('usage: runner contract-diff [verify] CELL PINYIN_SO [ZHUYIN_SO] [-- OPTIONS]', error=True)
            return 2
        return verify_contract(args)
    if recipe not in RECIPES:
        emit(f'unknown runner recipe: {recipe}', error=True)
        return 2
    def call() -> int:
        with ExitStack() as stack:
            settings = Settings.read(os.environ)
            context = pool.UnitContext(recipe, slot.CANCELLED, environment=dict(settings.environment))
            ctx = Context(settings, DriverServices(settings, context), stack, Path.cwd(), recipe)
            return RECIPES[recipe](ctx, args)
    return guarded(call)


def guarded(call: Callable[[], int]) -> int:
    """Run CALL under the cancellation handlers and return its status.

    The failures every runner shares end in the same statuses and messages: a
    finished or refused run, a cancellation, a missing
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
    except FileNotFoundError as error:
        emit(str(error), error=True)
        return 127
    except (OSError, ValueError, RuntimeError) as error:
        emit(f'FAIL: {error}', error=True)
        return 1
