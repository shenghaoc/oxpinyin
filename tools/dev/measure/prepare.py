"""Input preparation for the Valgrind recipes on a native Linux host (#1290).

`python -m tools.dev.measure.prepare oracle|callgrind` prepares what
callgrind-differential and system-import-profile read: oracle builds only the
selected pin cells with tools/oracle/build.py; callgrind also builds the profiling
and shipping arms over the pin's data. Every input is working data under the
`measure` entry of the cache root, and every build uses the vcpkg overlay tkrzw.
"""
import argparse
import contextlib
import hashlib
import io
import os
from pathlib import Path
import shutil
import sys

from .. import vcpkg
from ..runners import Finished, emit
from . import bench
from .bench import fail

DATA_SUBDIRECTORY = 'lib/libpinyin/data'
# The name the pin's tkrzw cell needs; tools/tkrzw/runtime.py certifies the overlay's file of this name.
TKRZW_SONAME = 'libtkrzw.so.1'


def measure_root(env):
    """The `measure` entry of the cache root, which holds every prepared measurement input."""
    from tools import cachedir
    return cachedir.root(env) / 'measure'


def pin_prefix(env, backend):
    """The prepared pin prefix of BACKEND."""
    return measure_root(env) / 'pinyin-oracle' / backend


def subject_stage(env, name):
    """The staged cargo-c install of the subject arm NAME (tkrzw: profiled, nodebug: shipping)."""
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


def build_pin(prefix, backend, extracted, work, env):
    """Build the pin's libraries and data for BACKEND into the empty PREFIX with tools/oracle/build.py.

    The libraries are linked as the pin's own recipe links them, with no run path.
    The pin's configure finds the overlay tkrzw through pkg-config, then links a bare -ltkrzw, and
    its Makefile.am does not use TKRZW_CFLAGS, so only default compiler and linker directories
    satisfy it. For the tkrzw cell, both build.py calls therefore get a CPATH that starts with the
    overlay's include directory and a LIBRARY_PATH that starts with its lib directory; build.py
    keeps both, and neither changes the compiler or linker flags.
    build.py replaces LD_LIBRARY_PATH with the prefix's own library directories, so the tkrzw
    cell's data generators would not find the overlay libtkrzw. For the data build only, the
    prefix's lib directory therefore holds a symbolic link to the overlay's libtkrzw.so.1; the
    link is removed afterwards, so the prefix holds only what build.py wrote.
    """
    env = vcpkg.pin_build_environment(env, backend)
    shutil.rmtree(prefix, ignore_errors=True)
    shutil.rmtree(work, ignore_errors=True)
    prefix.parent.mkdir(parents=True, exist_ok=True)
    common = [sys.executable, bench.ROOT / 'tools/oracle/build.py', '--dbm', backend, '--enable-libzhuyin',
              '--prefix', prefix]
    bench.heavy([*common, '--libs-only', '--work-dir', work / 'libs', '--jobs', str(os.cpu_count() or 1)], env=env)
    link = prefix / 'lib' / TKRZW_SONAME
    if backend == 'tkrzw':
        link.symlink_to(Path(env['OXPINYIN_TKRZW_PREFIX']) / 'lib' / TKRZW_SONAME)
    try:
        bench.heavy([*common, '--regen-data', '--model-dir', extracted, '--work-dir', work / 'data'], env=env)
    finally:
        if backend == 'tkrzw':
            link.unlink(missing_ok=True)


def prepare(argv, env):
    """Prepare the local inputs on this host, under the cache root."""
    from .recipes import require_local_measurement
    from .. import model
    require_local_measurement(env)
    parser = argparse.ArgumentParser(prog='python -m tools.dev.measure.prepare')
    parser.add_argument('mode', choices=('oracle', 'callgrind'))
    parser.add_argument('--cells', nargs='+', choices=('tkrzw', 'kc', 'bdb'), default=None)
    args = parser.parse_args(argv)
    if args.cells is not None and args.mode != 'oracle':
        parser.error('--cells is accepted only in oracle mode')
    cells = (args.cells or ['tkrzw', 'kc']) if args.mode == 'oracle' else ['tkrzw']
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
    # Build and certify the subject against the same overlay as the pin.
    env = vcpkg.environment(env)
    stages = {}
    for name, debug in (('tkrzw', True), ('nodebug', False)):
        stage = subject_stage(env, name)
        shutil.rmtree(stage, ignore_errors=True)
        build_env = dict(env, CARGO_TARGET_DIR=str(measure_root(env) / 'target' / name))
        if debug:
            build_env['RUSTFLAGS'] = '-Cdebuginfo=1 -Cstrip=none'
        else:
            build_env.pop('RUSTFLAGS', None)
        bench.heavy(['cargo', 'cinstall', '--locked', '--release', '-p', 'oxpinyin-capi',
                     '--no-default-features', '--features', 'tkrzw', f'--destdir={stage}',
                     '--prefix=/usr'], env=build_env)
        stages[name] = stage
    harness = harness_path(env)
    harness.parent.mkdir(parents=True, exist_ok=True)
    bench.run(bench.harness_build(harness, flags=(*bench.STRICT_FLAGS, '-g')))
    from . import valgrind
    status = valgrind.callgrind_control([str(stages['tkrzw']), str(stages['nodebug'])], env)
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
    """One native default-backend release build in a private invocation worktree.

    Linux packages the shipping library as macOS does. SYMBOLS adds the
    profiled arm's debug information; a Linux comparison builds without it.
    """
    import time
    from . import identity
    from .. import packaging
    if linux:
        # The managed checkout under the cache root, or an explicit VCPKG_ROOT.
        from .. import cell
        triplet, installed = vcpkg.triplet(), vcpkg.prefix(env)
        # Compilation, the loader and the libtkrzw certification all read the overlay install.
        env = vcpkg.environment(env)
    else:
        if not env.get('VCPKG_ROOT'):
            raise ValueError('native measurement requires VCPKG_ROOT; see the setup runbook')
        triplet = 'x64-windows' if windows else 'arm64-osx-oxpinyin'
        installed = Path(env['VCPKG_ROOT']).resolve() / 'installed' / triplet
    pkgconfig = installed / 'lib/pkgconfig'
    if not (pkgconfig / 'tkrzw.pc').is_file():
        raise ValueError(f'install tkrzw:{triplet} from the repository overlay before measuring')
    env = {**env, 'PKG_CONFIG_PATH': str(pkgconfig) +
           (os.pathsep + env['PKG_CONFIG_PATH'] if env.get('PKG_CONFIG_PATH') else '')}
    if windows:
        from tools.dev.check import msvc_environment
        env = msvc_environment(env)
        env.update(PKG_CONFIG=str(installed / 'tools/pkgconf/pkgconf.exe'),
                   PKG_CONFIG_PATH=str(pkgconfig), PKG_CONFIG_LIBDIR=str(pkgconfig))
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
        import_library = target / f'release/{library}.dll.lib'
        if windows:
            bench.run(['cargo', 'build', '--locked', '--release', '-p', package['name']],
                      cwd=source, env=build_env)
            dylib = target / f'release/{library}.dll'
            uuid = identity.sha256(dylib)
        else:
            # Use this tooling revision with each product source revision. No backend override.
            packaging.install_library(['libpinyin', '--prefix=' + str(prefix), '--',
                                       '--locked', '--release'],
                                      packaging.Tools(build_env, root=source))
            if linux:
                dylib = prefix / 'lib/libpinyin.so.15.0.0'
                uuid = identity.sha256(dylib)
                # The linked object names the backend; it must be Cargo's declared default.
                backend = identity.library_backend(dylib)
                if backend != cell.BACKEND_TO_CELL[cell.declared_default(source)]:
                    fail('packaged library does not link the default backend: ' + backend)
            else:
                dylib = prefix / 'lib/libpinyin.15.0.0.dylib'
                uuid = identity.macho_uuid(dylib)
                if symbols and identity.macho_uuid(str(dylib) + '.dSYM') != uuid:
                    fail('packaged dylib and dSYM UUID differ')
        return dict(revision=revision, prefix=prefix, dylib=dylib, uuid=uuid,
                    **(dict(linux=True, backend=backend) if linux else {}),
                    windows=windows, import_library=import_library,
                    default_features=package['features']['default'],
                    cold_build_seconds=time.monotonic() - started,
                    build_options={'profile': 'release', 'features': [],
                                   'RUSTFLAGS': build_env.get('RUSTFLAGS', ''),
                                   'OXPINYIN_DSYM': build_env.get('OXPINYIN_DSYM', ''),
                                   'toolchain': build_env.get('RUSTUP_TOOLCHAIN', ''),
                                   'PKG_CONFIG_PATH': build_env['PKG_CONFIG_PATH']})
    finally:
        bench.run(['git', 'worktree', 'remove', '--force', source])


def native_driver(arm, output, env):
    """Build one linked driver; Darwin also rewrites and signs its load command."""
    from . import identity
    output.parent.mkdir(parents=True, exist_ok=True)
    if arm.get('linux'):
        # The existing dlopen driver, built as the Callgrind recipes build it.
        bench.run(bench.harness_build(output, compiler='cc', flags=(*bench.STRICT_FLAGS, '-g')), env=env)
        return identity.sha256(output)
    if arm.get('windows'):
        from tools.dev.check import msvc_environment, lookup
        toolenv = msvc_environment(env)
        compiler = shutil.which('cl', path=lookup(toolenv, 'PATH'))
        bench.run([compiler, '/nologo', '/std:c11', '/O2', '/MD', '/W3', '/DMEASURE_LINKED',
                   '/Fe:' + str(output), '/Fo:' + str(output.with_suffix('.obj')),
                   bench.HARNESS_SOURCE, arm['import_library'], 'psapi.lib'], env=toolenv)
        return identity.sha256(output)

    bench.run(bench.harness_build(output, compiler='cc',
        flags=(*bench.STRICT_FLAGS, '-g', '-DMEASURE_LINKED'),
        libraries=(str(arm['dylib']), '-Wl,-rpath,@executable_path/../lib')), env=env)
    listing = bench.capture(['otool', '-D', arm['dylib']])
    if listing.status or len(listing.stdout.decode().splitlines()) != 2:
        fail('cannot read packaged install name')
    name = listing.stdout.decode().splitlines()[1].strip()
    bench.run(['install_name_tool', '-change', name, '@rpath/' + arm['dylib'].name, output])
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
