"""Input preparation for the Valgrind recipes inside Dockerfile.oracle's performance stage (#1290).

`python -m tools.dev.measure.prepare oracle|callgrind` prepares what
callgrind-differential and system-import-profile read: oracle prepares only the
selected pin cells; callgrind also builds the profiling, shipping and allocation
arms over the pin's data. Model data and subject artifacts are runtime working
data, never image layers.
"""
import argparse
import contextlib
import hashlib
import io
import os
from pathlib import Path
import shutil
import sys

from ..runners import Finished, emit
from . import bench
from .bench import fail

SCRATCH = Path('/performance/tmp')
DATA_SUBDIRECTORY = 'lib/libpinyin/data'


def running_inventory():
    """The running image's installed versions, from the versions.py code that records them."""
    from tools.oracle import versions
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        versions.main(['--print'])
    return output.getvalue()


def prepared_pin_refusal(prefix, backend):
    """What stops reuse of the prepared PREFIX, or None: prefix_ok for the libraries, the pinned
    model, the running image's installed versions, so pin and subject share one base, and both
    data manifests."""
    from tools.oracle.image import fields, model_ok, prefix_ok
    try:
        if not prefix_ok(prefix, backend):
            return 'prefix_ok refused its pin commits, recipe, backend, architecture or recorded library or versions bytes'
        recorded = fields(prefix / 'oracle-pin.txt')
        if not model_ok(prefix):
            return f"model_sha256 is {recorded.get('model_sha256')}, not the pinned model_archive_sha256"
        versions = (prefix / 'versions.txt').read_text()
        inventory = running_inventory()
        if versions != inventory:
            differ = sorted(set(versions.splitlines()) ^ set(inventory.splitlines()))
            return "versions.txt differs from the running image's inventory: " + '; '.join(differ)
        for manifest, key in (('oracle-data.sha256', 'data_manifest_sha256'),
                              ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')):
            content = (prefix / manifest).read_bytes()
            if hashlib.sha256(content).hexdigest() != recorded.get(key):
                return f'{manifest} differs from its recorded {key}'
            for line in content.decode().splitlines():
                digest, name = line.split('  ', 1)
                path = prefix / name
                if not path.resolve().is_relative_to(prefix.resolve()):
                    return f'{manifest} lists {name}, outside the prefix'
                if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                    return f'{name} differs from {manifest}'
        if not (prefix / DATA_SUBDIRECTORY / 'table.conf').is_file():
            return f'{DATA_SUBDIRECTORY}/table.conf is missing'
        return None
    except (OSError, ValueError, UnicodeError) as error:
        return str(error)


def prepared_pin_matches(prefix, backend):
    """Whether the prepared PREFIX may be reused."""
    return prepared_pin_refusal(prefix, backend) is None


def prepare(argv, env):
    """Prepare the local inputs after entering Dockerfile.oracle's performance stage."""
    from .recipes import require_performance_environment
    from .. import model
    require_performance_environment(env)
    parser = argparse.ArgumentParser(prog='python -m tools.dev.measure.prepare')
    parser.add_argument('mode', choices=('oracle', 'callgrind'))
    parser.add_argument('--cells', nargs='+', choices=('tkrzw', 'kc', 'bdb'), default=None)
    args = parser.parse_args(argv)
    if args.cells is not None and args.mode != 'oracle':
        parser.error('--cells is accepted only in oracle mode')
    cells = (args.cells or ['tkrzw', 'kc']) if args.mode == 'oracle' else ['tkrzw']
    extracted = model.fetch('/performance/model20', env=env)
    for backend in cells:
        seed = Path('/opt/pinyin-oracle') / backend
        prefix = Path('/performance/pinyin-oracle') / backend
        if not prepared_pin_matches(prefix, backend):
            shutil.rmtree(prefix, ignore_errors=True)
            prefix.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(seed, prefix, symlinks=True)
            bench.heavy([sys.executable, bench.ROOT / 'tools/oracle/build.py', '--regen-data',
                         '--dbm', backend, '--enable-libzhuyin', '--prefix', prefix, '--model-dir', extracted,
                         '--work-dir', f'/performance/oracle-{backend}'], env=env)
            refusal = prepared_pin_refusal(prefix, backend)
            if refusal:
                fail(f'preparation: the rebuilt {backend} prefix {prefix} does not match: {refusal}')
        alias = Path('/opt/libpinyin-' + backend)
        if alias.is_symlink():
            alias.unlink()
        elif alias.exists():
            shutil.rmtree(alias)
        alias.symlink_to(prefix, target_is_directory=True)
    if args.mode == 'oracle':
        return 0
    for name, features, debug in (('tkrzw', 'tkrzw', True), ('nodebug', 'tkrzw', False),
                                  ('alloccount', 'tkrzw,alloc-count', True)):
        installed = Path(f'/performance/oxpinyin-{name}')
        installed.mkdir(parents=True, exist_ok=True)
        alias = Path(f'/opt/oxpinyin-{name}')
        if not alias.exists():
            alias.symlink_to(installed, target_is_directory=True)
        build_env = dict(env)
        if debug:
            build_env['RUSTFLAGS'] = '-Cdebuginfo=1 -Cstrip=none'
        else:
            build_env.pop('RUSTFLAGS', None)
        bench.heavy(['cargo', 'cinstall', '--locked', '--release', '-p', 'oxpinyin-capi',
                     '--no-default-features', '--features', features, f'--destdir={installed / "stage"}',
                     '--prefix=/usr'], env=build_env)
    harness = SCRATCH / 'bisect'
    bench.run(bench.harness_build(harness, flags=(*bench.STRICT_FLAGS, '-g')))
    from . import valgrind
    status = valgrind.callgrind_control(['/opt/oxpinyin-tkrzw/stage', '/opt/oxpinyin-nodebug/stage'], env)
    if status:
        raise Finished(status)
    status, report = valgrind.anchors_table(harness)
    emit(report)
    if status:
        raise Finished(status)
    return 0


if __name__ == '__main__':
    from ..runners import guarded
    raise SystemExit(guarded(lambda: prepare(sys.argv[1:], dict(os.environ))))
