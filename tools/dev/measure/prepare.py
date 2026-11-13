"""Input preparation for the Valgrind recipes inside Dockerfile.oracle's performance stage (#1290).

`python -m tools.dev.measure.prepare oracle|callgrind` prepares what
callgrind-differential and system-import-profile read: oracle prepares only the
selected pin cells; callgrind also builds the profiling and shipping arms over
the pin's data. Model data and subject artifacts are runtime working
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
    extracted = model.fetch(env=env)
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
    for name, debug in (('tkrzw', True), ('nodebug', False)):
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
                     '--no-default-features', '--features', 'tkrzw', f'--destdir={installed / "stage"}',
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


def native_build(revision, destination, env, *, symbols=True, windows=False, linux=False):
    """One native default-backend release build in a private invocation worktree.

    Linux packages the shipping library as macOS does. SYMBOLS adds the
    profiled arm's debug information; a Linux comparison builds without it.
    """
    import json
    import time
    from . import identity
    from .. import packaging
    if linux:
        # The managed checkout under the cache root, or an explicit VCPKG_ROOT.
        from .. import cell, vcpkg
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
        if windows:
            bench.run(['cargo', 'build', '--locked', '--release', '-p', 'oxpinyin-capi'],
                      cwd=source, env=build_env)
            dylib = target / 'release/pinyin_capi.dll'
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
        metadata = bench.capture(['cargo', 'metadata', '--locked', '--no-deps', '--format-version=1'],
                                 cwd=source, env=build_env)
        if metadata.status:
            fail('cargo metadata failed')
        package = next(item for item in json.loads(metadata.stdout)['packages'] if item['name'] == 'oxpinyin-capi')
        return dict(revision=revision, prefix=prefix, dylib=dylib, uuid=uuid,
                    **(dict(linux=True, backend=backend) if linux else {}),
                    windows=windows, import_library=target / 'release/pinyin_capi.dll.lib',
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
