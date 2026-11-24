"""Input preparation for the Valgrind recipes on a native Linux host (#1290).

`python -m tools.dev.measure.prepare oracle|callgrind` prepares what
callgrind-differential and system-import-profile read: oracle builds only the
selected pin cells with tools/oracle/build.py; callgrind also builds the profiling
and shipping arms over the pin's data. Every input is working data under the
`measure` entry of the cache root. On Linux every build uses the local cell,
Kyoto Cabinet from the distribution (LOCAL_CELL in tools/dev/cell.py;
docs/runbooks/setup.md#linux).
"""
import argparse
import contextlib
import hashlib
import io
import os
from pathlib import Path
import shutil
import sys

from .. import cell
from ..cell import LOCAL_CELL
from ..runners import Finished, emit
from . import bench
from .bench import fail
from tools.oracle.build import file_sha256

DATA_SUBDIRECTORY = 'lib/libpinyin/data'


def measure_root(env):
    """The `measure` entry of the cache root, which holds every prepared measurement input."""
    from tools import cachedir
    return cachedir.root(env) / 'measure'


def pin_prefix(env, backend):
    """The prepared pin prefix of BACKEND."""
    return measure_root(env) / 'pinyin-oracle' / backend


def stage_name(cell_name, *, debug):
    """The stage name of the subject arm of CELL_NAME: the local cell keeps its historical names."""
    if cell_name == LOCAL_CELL:
        return LOCAL_CELL if debug else 'nodebug'
    return cell_name if debug else f'nodebug-{cell_name}'


def subject_stage(env, name):
    """The staged cargo-c install of the subject arm NAME (the local cell: profiled, nodebug: shipping)."""
    return measure_root(env) / f'oxpinyin-{name}' / 'stage'


def harness_path(env):
    """The bisect driver that the preparation builds and the differential runs."""
    return measure_root(env) / 'tmp' / 'bisect'


def results(env, recipe):
    """The default output directory of RECIPE."""
    return measure_root(env) / 'results' / recipe


def running_inventory():
    """This host's installed versions, from the versions.py code that records them."""
    from tools.oracle import versions
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        versions.main(['--print'])
    return output.getvalue()


def prepared_pin_refusal(prefix, backend):
    """What stops reuse of the prepared PREFIX, or None: prefix_ok for the libraries, the pinned
    model, this host's installed versions, so pin and subject share one base, and both
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
            return "versions.txt differs from this host's inventory: " + '; '.join(differ)
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
                if file_sha256(path) != digest:
                    return f'{name} differs from {manifest}'
        if not (prefix / DATA_SUBDIRECTORY / 'table.conf').is_file():
            return f'{DATA_SUBDIRECTORY}/table.conf is missing'
        return None
    except (OSError, ValueError, UnicodeError) as error:
        return str(error)


def prepared_pin_matches(prefix, backend):
    """Whether the prepared PREFIX may be reused."""
    return prepared_pin_refusal(prefix, backend) is None


def build_pin(prefix, backend, extracted, work, env):
    """Build the pin's libraries and data for BACKEND into the empty PREFIX with tools/oracle/build.py.

    The libraries are linked as the pin's own recipe links them, with no run path. The backend's
    development package is the distribution's, so the pin's configure finds it on the default
    pkg-config, include and library paths, and its data generators load the library from the
    default loader path after build.py replaces LD_LIBRARY_PATH with the prefix's own directories.
    """
    shutil.rmtree(prefix, ignore_errors=True)
    shutil.rmtree(work, ignore_errors=True)
    prefix.parent.mkdir(parents=True, exist_ok=True)
    common = [sys.executable, bench.ROOT / 'tools/oracle/build.py', '--dbm', backend, '--enable-libzhuyin',
              '--prefix', prefix]
    bench.run([*common, '--libs-only', '--work-dir', work / 'libs', '--jobs', str(os.cpu_count() or 1)], env=env)
    bench.run([*common, '--regen-data', '--model-dir', extracted, '--work-dir', work / 'data'], env=env)


def prepare(argv, env):
    """Prepare the local inputs on this host, under the cache root."""
    from .recipes import require_local_measurement
    from .. import model
    require_local_measurement(env)
    parser = argparse.ArgumentParser(prog='python -m tools.dev.measure.prepare')
    parser.add_argument('mode', choices=('oracle', 'callgrind'))
    # The default Linux measurement cell is the local one; bdb and tkrzw are built on request.
    parser.add_argument('--cells', nargs='+', choices=cell.CELLS, default=None)
    args = parser.parse_args(argv)
    cells = args.cells or [LOCAL_CELL]
    extracted = model.fetch(env=env)
    for backend in cells:
        prefix = pin_prefix(env, backend)
        if not prepared_pin_matches(prefix, backend):
            build_pin(prefix, backend, extracted, measure_root(env) / f'oracle-{backend}', env)
            refusal = prepared_pin_refusal(prefix, backend)
            if refusal:
                fail(f'preparation: the rebuilt {backend} prefix {prefix} does not match: {refusal}')
        emit(f'{backend}: pin prefix {prefix}')
    if args.mode == 'oracle':
        return 0
    # Build the subject on the same cell as the pin, one pair of arms per requested cell.
    harness = harness_path(env)
    harness.parent.mkdir(parents=True, exist_ok=True)
    bench.build_harness(harness, env=env)
    from . import valgrind
    for cell_name in cells:
        stages = {}
        for debug in (True, False):
            name = stage_name(cell_name, debug=debug)
            stage = subject_stage(env, name)
            shutil.rmtree(stage, ignore_errors=True)
            build_env = dict(env, CARGO_TARGET_DIR=str(measure_root(env) / 'target' / name))
            if debug:
                build_env['RUSTFLAGS'] = '-Cdebuginfo=1 -Cstrip=none'
            else:
                build_env.pop('RUSTFLAGS', None)
            bench.run(['cargo', 'cinstall', '--locked', '--release', '-p', 'oxpinyin-capi',
                       '--no-default-features', '--features', cell.CELL_TO_BACKEND[cell_name],
                       f'--destdir={stage}', '--prefix=/usr'], env=build_env)
            stages[debug] = stage
        emit(f'{cell_name}: subject arms {stages[True]} and {stages[False]}')
        status = valgrind.callgrind_control([str(stages[True]), str(stages[False])], env)
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


def native_build(revision, destination, env, *, symbols=True, windows=False, linux=False):
    """One native release build in a private invocation worktree.

    macOS and Windows build Cargo's default backend: tkrzw from Homebrew, or the repository's
    static build (`just tkrzw`). Linux builds the local cell, Kyoto Cabinet from the distribution,
    and packages the shipping library as macOS does. SYMBOLS adds the profiled arm's debug
    information; a Linux comparison builds without it.
    """
    import time
    from . import identity
    from .. import packaging
    features = []
    if linux:
        features = ['--no-default-features', '--features', cell.CELL_TO_BACKEND[LOCAL_CELL]]
        if bench.capture(['pkg-config', '--exists', 'kyotocabinet'], env=env).status:
            raise ValueError('install libkyotocabinet-dev before measuring: pkg-config finds no kyotocabinet')
    elif windows:
        env = windows_environment(env, bench.VCPKG_INSTALLED)
    else:
        from ..provision import homebrew_tkrzw_prefix
        pkgconfig = homebrew_tkrzw_prefix(env) / 'lib/pkgconfig'
        if not (pkgconfig / 'tkrzw.pc').is_file():
            raise ValueError(f'missing tkrzw.pc in {pkgconfig}')
    source = destination / 'source'
    prefix = destination / 'prefix'
    target = destination / 'target'
    destination.mkdir(parents=True)
    bench.run(['git', 'worktree', 'add', '--detach', source, revision])
    build_env = {key: value for key, value in env.items()
                 if key not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')
                 and not key.startswith('CARGO_PROFILE_')}
    build_env.update(CARGO_TARGET_DIR=str(target))
    build_env.pop('OXPINYIN_DSYM', None)
    if symbols and not windows:
        build_env.update(RUSTFLAGS='-Cdebuginfo=1 -Cstrip=none')
        if not linux:
            build_env.update(OXPINYIN_DSYM='1')
    started = time.monotonic()
    try:
        # The product revision's own metadata names its C crate and that crate's library target.
        package = packaging.capi_package(source, 'pinyin', build_env)
        library = next(item['name'] for item in package['targets']
                       if 'cdylib' in item['crate_types']).replace('-', '_')
        if windows:
            bench.run(['cargo', 'build', '--locked', '--release', '-p', package['name']],
                      cwd=source, env=build_env)
            dylib = target / f'release/{library}.dll'
            uuid = identity.sha256(dylib)
        else:
            # Use this tooling revision with each product source revision. Linux selects the local
            # cell; macOS builds the revision's default.
            packaging.install_library(['libpinyin', '--prefix=' + str(prefix), '--',
                                       '--locked', '--release', *features],
                                      packaging.Tools(build_env, root=source))
            if linux:
                dylib = prefix / 'lib/libpinyin.so.15.0.0'
                uuid = identity.sha256(dylib)
                # The linked object names the backend; it must be the local cell.
                backend = identity.library_backend(dylib)
                if backend != LOCAL_CELL:
                    fail('packaged library does not link the local backend: ' + backend)
            else:
                dylib = prefix / 'lib/libpinyin.15.0.0.dylib'
                uuid = identity.macho_uuid(dylib)
                if symbols and identity.macho_uuid(str(dylib) + '.dSYM') != uuid:
                    fail('packaged dylib and dSYM UUID differ')
        return dict(revision=revision, prefix=prefix, dylib=dylib, uuid=uuid,
                    **(dict(linux=True, backend=backend) if linux else {}),
                    windows=windows,
                    default_features=package['features']['default'],
                    cold_build_seconds=time.monotonic() - started,
                    build_options={'profile': 'release', 'features': features[2:],
                                   'RUSTFLAGS': build_env.get('RUSTFLAGS', ''),
                                   'OXPINYIN_DSYM': build_env.get('OXPINYIN_DSYM', ''),
                                   'toolchain': build_env.get('RUSTUP_TOOLCHAIN', ''),
                                   'PKG_CONFIG_PATH': build_env.get('PKG_CONFIG_PATH', '')})
    finally:
        bench.run(['git', 'worktree', 'remove', '--force', source])


def windows_environment(env, installed):
    """The Windows arm's build environment: MSVC, vcpkg's pkgconf and GLib, and the static tkrzw first.

    Both arms read one pkg-config search path, so they link the same GLib
    import library and the same repository-built tkrzw archive.
    """
    from tools.dev.check import msvc_environment
    from tools.tkrzw import windows as tkrzw
    pkgconfig = installed / 'lib/pkgconfig'
    if not (pkgconfig / 'glib-2.0.pc').is_file():
        raise ValueError(f'run vcpkg install --triplet x64-windows in the repository root before measuring: no glib-2.0.pc in {pkgconfig}')
    revision = tkrzw.pin()['revision']
    prefix = tkrzw.default_prefix(env, revision)
    if not tkrzw.built(prefix, revision):
        raise ValueError(f'run just tkrzw before measuring: no complete tkrzw build in {prefix}')
    search = tkrzw.environment(prefix, {'PKG_CONFIG_PATH': str(pkgconfig)})['PKG_CONFIG_PATH']
    env = msvc_environment(env)
    env.update(PKG_CONFIG=str(installed / 'tools/pkgconf/pkgconf.exe'),
               PKG_CONFIG_PATH=search, PKG_CONFIG_LIBDIR=search)
    return env


def native_driver(arm, output, env):
    """Build the dynamically loaded driver; Darwin signs it for Instruments."""
    from . import identity
    output.parent.mkdir(parents=True, exist_ok=True)
    bench.build_harness(output, env=env)
    if arm.get('linux') or arm.get('windows'):
        return identity.sha256(output)

    import plistlib
    import tempfile
    import re
    # Instruments needs task_control_t for Allocations. This entitlement belongs
    # only to the local driver; never re-sign the measured product dylib.
    with tempfile.TemporaryDirectory(prefix='driver-sign-') as directory:
        entitlements = Path(directory) / 'entitlements.plist'
        entitlements.write_bytes(plistlib.dumps({'com.apple.security.get-task-allow': True}))
        bench.run(['codesign', '--force', '--sign', '-', '--options', '0',
                   '--entitlements', entitlements, output])
    bench.run(['codesign', '--verify', '--strict', output])
    signature = bench.capture(['codesign', '-d', '--entitlements', '-', '--xml', '--verbose=4', output])
    flags = re.search(r'\bflags=0x([0-9a-fA-F]+)\b', signature.stderr.decode())
    if (signature.status or not flags or int(flags[1], 16) & 0x10000
            or plistlib.loads(signature.stdout).get('com.apple.security.get-task-allow') is not True):
        fail('driver must allow debugging without hardened runtime')
    return identity.sha256(output)
