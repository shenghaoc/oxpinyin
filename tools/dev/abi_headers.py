"""Compile each public header as C++ and verify the library its executable loads."""
from functools import partial
from pathlib import Path
import os
import re
import shlex
import subprocess

from . import host, pool, slot
from .cell import Settings
from .drivers import DriverServices
from .packaging import cargo_metadata, crate_dir


def check_headers(settings: Settings, work: Path, libraries: dict[str, Path]) -> None:
    flags = settings.environment.get('GLIB_CFLAGS') or subprocess.check_output(
        ['pkg-config', '--cflags', 'glib-2.0'], env=settings.environment, text=True)
    metadata = cargo_metadata(settings.root, settings.environment)
    compiler = shlex.split(settings.environment.get('CXX') or 'c++')

    def check(name: str, library: Path, context: pool.UnitContext) -> None:
        def run(command, *, environment=settings.environment):
            result = context.launch([str(arg) for arg in command], env=environment, merge_stderr=True)
            output = result.stdout.decode(errors='replace')
            if result.status:
                raise ValueError(f'{name}.h: {command[0]} exited {result.status}\n{output}')
            return output

        directory = work / name
        directory.mkdir()
        soname = f'lib{name}.so.15'
        (directory / soname).symlink_to(library)
        executable = directory / 'header'
        include = crate_dir(settings.root, name, metadata=metadata)
        run([*compiler, '-std=c++17', '-Wall', '-Wextra', '-Werror', *shlex.split(flags),
             '-I', include, settings.root / 'tools/abi' / f'{name}.cc', library,
             f'-Wl,-rpath,{directory}', '-o', executable])
        dynamic = run(['readelf', '-d', executable])
        needed = re.findall(r'\(NEEDED\).*?\[(lib(?:ox)?(?:pinyin|zhuyin)[^]]*)\]', dynamic)
        if needed != [soname]:
            raise ValueError(f'{name}.h: expected DT_NEEDED {soname}, found {needed}')
        resolved = run(['ldd', executable], environment=dict(settings.environment, LD_LIBRARY_PATH=''))
        match = re.search(rf'^\s*{re.escape(soname)} => (.+?) \(0x', resolved, flags=re.M)
        if not match or Path(match[1]) != directory / soname or not Path(match[1]).samefile(library):
            raise ValueError(f'{name}.h: {soname} did not resolve to {library}\n{resolved}')
        # Keep the facade we just resolved first when the host needs search paths
        # for the facade's own native dependencies.
        loader_path = os.pathsep.join(filter(None, [str(directory), settings.environment.get('LD_LIBRARY_PATH')]))
        DriverServices(settings, context).run(
            [str(executable)], executable, runner=f'{name}.h', side='subject', cwd=settings.root,
            environment=dict(settings.environment, LD_LIBRARY_PATH=loader_path), capture=False,
        ).check_returncode()

    units = [(name, partial(check, name, library)) for name, library in libraries.items()]
    pool.run_units(units, min(len(units), len(host.allowed_cpus())), cancelled=slot.CANCELLED.is_set,
                   cancel_error=slot.Cancelled)
