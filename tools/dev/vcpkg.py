"""vcpkg for development and for the CI jobs; the overlay port alone owns the tkrzw pin.

Development: `install` puts the overlay port into an explicit or managed
checkout, and `environment` selects that install for compilation, execution
and certification.

CI: `bootstrap` fetches the pinned checkout where the runner has none (the
Debian container jobs). `cache-key` finds the checkout, names its revision and
publishes the binary cache location and key. `install` installs the requested
packages with the repository overlay and publishes where pkg-config, the loader
and PATH find them. `cache-report` prints what the cache restore matched. A
workflow keeps only the cache restore and save actions between these commands.

The CI commands use the standard library only: the container jobs run them with
the image's own Python, before any managed interpreter exists.
"""
import argparse
import errno
import hashlib
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace

from tools import cachedir

ROOT = Path(__file__).resolve().parents[2]
# Match the vcpkg revision verified by the macOS migration.
REVISION = '2ec0f1747d8186a5c9babe276e2ad57cd2b12d2a'


def triplet():
    """Select the native overlay triplet before touching an install."""
    if platform.system() == 'Darwin':
        return 'arm64-osx-oxpinyin'
    architecture = platform.machine().lower()
    targets = {'x86_64': 'x64', 'amd64': 'x64', 'aarch64': 'arm64', 'arm64': 'arm64'}
    if architecture not in targets:
        raise ValueError(f'unsupported Linux architecture for tkrzw: {architecture}; expected x86_64 or arm64')
    return f'{targets[architecture]}-linux-oxpinyin'


def checkout(env):
    """Use an explicit checkout, or keep one outside Git worktrees in the cache."""
    return Path(env['VCPKG_ROOT']).resolve() if env.get('VCPKG_ROOT') else (cachedir.root(env) / 'vcpkg').resolve()


def prefix(env):
    return checkout(env) / 'installed' / triplet()


def executable(root):
    """The vcpkg program of the checkout at ROOT, or None when it has none."""
    for name in ('vcpkg.exe', 'vcpkg'):
        if (root / name).is_file():
            return root / name
    return None


def overlay_options(root):
    """The repository's overlay ports and triplets, for every vcpkg command."""
    return [f'--overlay-ports={root / "vcpkg-overlays"}', f'--overlay-triplets={root / "triplets"}']


def installed_environment(env, installed):
    """Put the install at INSTALLED first for compilation, execution and certification."""
    child = dict(env, OXPINYIN_TKRZW_PREFIX=str(installed))
    # A previously sourced environment must not certify a different install.
    child.pop('OXPINYIN_TKRZW_LIBRARY', None)
    child.pop('OXPINYIN_TKRZW_PREFIXES', None)
    for name, path in (('PKG_CONFIG_PATH', installed / 'lib/pkgconfig'),
                       ('LD_LIBRARY_PATH', installed / 'lib'),
                       ('PATH', installed / 'tools/tkrzw/bin')):
        child[name] = str(path) + (os.pathsep + env[name] if env.get(name) else '')
    return child


def environment(env):
    """Put one overlay install first for compilation, execution and certification."""
    return installed_environment(env, prefix(env))


def fetch_checkout(destination, env, *, run, cwd):
    """Fetch vcpkg at REVISION into DESTINATION and bootstrap it there."""
    run(['git', 'init', str(destination)], env=env, cwd=cwd)
    run(['git', 'fetch', '--depth=1', 'https://github.com/microsoft/vcpkg.git', REVISION], env=env, cwd=destination)
    run(['git', 'checkout', '--detach', 'FETCH_HEAD'], env=env, cwd=destination)
    run(['bash', str(destination / 'bootstrap-vcpkg.sh'), '-disableMetrics'],
        env=dict(env, VCPKG_ROOT=str(destination)), cwd=destination)


def install(env, root, *, jobs):
    """Install using vcpkg's ABI-checked binary cache, without a second source builder."""
    from .process import run
    target = triplet()
    vcpkg = checkout(env)
    if env.get('VCPKG_ROOT'):
        # The caller owns the checkout; only vcpkg may update its packages.
        if executable(vcpkg) is None:
            raise ValueError(f'VCPKG_ROOT needs a bootstrapped vcpkg checkout: {vcpkg}')
    elif not vcpkg.exists():
        vcpkg.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.vcpkg-', dir=vcpkg.parent) as directory:
            staged = Path(directory) / 'checkout'
            fetch_checkout(staged, env, run=run, cwd=root)
            try:
                staged.rename(vcpkg)
            except OSError as error:
                if error.errno not in (errno.EEXIST, errno.ENOTEMPTY) or executable(vcpkg) is None:
                    raise
    program = executable(vcpkg)
    if program is None:
        raise ValueError(f'incomplete managed vcpkg checkout: {vcpkg}; remove it and retry')
    cache = cachedir.root(env) / 'vcpkg-binary-cache'
    cache.mkdir(parents=True, exist_ok=True)
    child = dict(env, VCPKG_ROOT=str(vcpkg), VCPKG_MAX_CONCURRENCY=str(jobs),
                 VCPKG_BINARY_SOURCES=env.get('VCPKG_BINARY_SOURCES', f'clear;files,{cache},readwrite'))
    options = ['--classic', *overlay_options(root)]
    run([program, 'install', f'tkrzw:{target}', *options],
        env=child, cwd=root, heavy=True)
    # Classic install leaves an installed older version untouched; upgrade replaces it.
    run([program, 'upgrade', f'tkrzw:{target}', '--no-dry-run', *options],
        env=child, cwd=root, heavy=True)


def runner_checkout(env, *, which=shutil.which):
    """A CI job's vcpkg checkout: the image's named root, the bootstrapped one, or the `vcpkg` on PATH."""
    named = env.get('VCPKG_INSTALLATION_ROOT') or env.get('VCPKG_ROOT')
    if named:
        root = Path(named)
    else:
        found = which('vcpkg', path=env.get('PATH'))
        if not found:
            raise ValueError('no vcpkg checkout: VCPKG_INSTALLATION_ROOT and VCPKG_ROOT are unset '
                             'and vcpkg is not on PATH')
        root = Path(os.path.realpath(found)).parent
    if executable(root) is None:
        raise ValueError(f"no vcpkg checkout at '{root}'")
    return root


def revision(root, *, capture):
    """The checkout's commit, or a digest of its baseline when it carries no Git metadata."""
    done = capture(['git', '-C', str(root), 'rev-parse', 'HEAD'])
    if not done.status and done.stdout.strip():
        return done.stdout.decode().strip()
    baseline = hashlib.sha256((root / 'versions/baseline.json').read_bytes()).hexdigest()[:40]
    return 'baseline-' + baseline


def binary_cache(env, triplet, packages='', *, capture, which=shutil.which, os_release=Path('/etc/os-release')):
    """The environment and step outputs that select the binary cache for TRIPLET.

    The key changes with the checkout revision and with the toolchain. On a
    hosted Windows or macOS runner the image names the toolchain. A Linux
    container job names its own compiler and distribution: a rolling image
    changes them without changing the runner image. A macOS or Linux key also
    names the checkout path, which the installed libtkrzw and tkrzw.pc record,
    so an archive built under another root is not reused. PACKAGES, the
    space-separated specs a job installs, adds a `packages` output for the
    workflow to append: a different request is a different cache entry.
    Paths are published with forward slashes; the Windows steps run in bash.
    """
    linux = 'linux' in triplet
    for name in ('RUNNER_TEMP',) if linux else ('ImageOS', 'ImageVersion', 'RUNNER_TEMP'):
        if not env.get(name):
            raise ValueError(f'runner variable {name} is unset')
    root = runner_checkout(env, which=which)
    directory = (Path(env['RUNNER_TEMP']) / 'vcpkg-binary-cache').as_posix()
    name = revision(root, capture=capture)
    if linux:
        compiler = capture(['c++', '--version'])
        if compiler.status:
            raise ValueError('cannot identify the C++ compiler: c++ --version failed')
        identity = compiler.stdout + os_release.read_bytes() + (root.as_posix() + '\n').encode()
        prefix = f'vcpkg-bin-Linux-{triplet}-'
        key = f'{prefix}{name}-{hashlib.sha256(identity).hexdigest()[:16]}'
    else:
        prefix = f"vcpkg-bin-{env['ImageOS']}-{triplet}-"
        if 'osx' in triplet:
            prefix += hashlib.sha256(root.as_posix().encode()).hexdigest()[:16] + '-'
        key = f"{prefix}{name}-{env['ImageOS']}-{env['ImageVersion']}"
    outputs = {'dir': directory, 'prefix': prefix, 'key': key,
               'installed': (root / 'installed' / triplet).as_posix()}
    if packages.split():
        outputs['packages'] = hashlib.sha256(packages.encode()).hexdigest()[:16]
    return dict(
        environment={'VCPKG_ROOT': root.as_posix(), 'VCPKG_BINARY_SOURCES': f'clear;files,{directory},readwrite'},
        outputs=outputs)


def publish(path, values):
    """Append NAME=VALUE lines to a GitHub Actions command file."""
    with open(path, 'a', encoding='utf-8') as stream:
        for name, value in values.items():
            if '\n' in name + value:
                raise ValueError('a published value holds a newline: ' + name)
            stream.write(f'{name}={value}\n')


def runner_install(env, triplet, packages='', *, run, root=ROOT):
    """Install PACKAGES for TRIPLET with the overlay, check them, and return what later steps need.

    PACKAGES defaults to tkrzw alone. The result names the environment
    variables to publish and the directories to put on PATH.
    """
    if not env.get('VCPKG_ROOT'):
        raise ValueError('VCPKG_ROOT is unset; run cache-key first')
    vcpkg = Path(env['VCPKG_ROOT'])
    program = executable(vcpkg)
    if program is None:
        raise ValueError(f"no vcpkg checkout at '{vcpkg}'")
    run([program, 'install', *(packages.split() or [f'tkrzw:{triplet}']), *overlay_options(root)], env=env, cwd=root)
    installed = vcpkg / 'installed' / triplet
    own = (installed / 'lib/pkgconfig').as_posix()
    if 'windows' in triplet:
        # vcpkg's own pkgconf answers for the installed tree; the DLLs load from its bin.
        pkg_config = installed / 'tools/pkgconf/pkgconf.exe'
        run([pkg_config, '--modversion', '--cflags', '--libs', 'glib-2.0', 'tkrzw'],
            env=dict(env, PKG_CONFIG_PATH=own), cwd=root)
        return dict(environment={'PKG_CONFIG': pkg_config.as_posix(), 'PKG_CONFIG_PATH': own},
                    path=[(installed / 'bin').as_posix()])
    if 'osx' not in triplet and 'linux' not in triplet:
        raise ValueError(f'no install rule for triplet {triplet}')
    # The installed library must answer pkg-config by itself.
    run(['pkg-config', '--modversion', '--cflags', '--libs', 'tkrzw'], env=dict(env, PKG_CONFIG_PATH=own), cwd=root)
    tools = [(installed / 'tools/tkrzw/bin').as_posix()]
    if 'osx' in triplet:
        # It must also name its own path: the binaries load it with no rpath.
        run(['otool', '-D', installed / 'lib/libtkrzw.dylib'], env=env, cwd=root)
        inherited = os.pathsep + env['PKG_CONFIG_PATH'] if env.get('PKG_CONFIG_PATH') else ''
        return dict(environment={'PKG_CONFIG_PATH': own + inherited}, path=tools)
    # Linux: the selection a developer's environment makes, certified against this prefix.
    selected = installed_environment(env, installed)
    return dict(environment={name: selected[name] for name in
                             ('PKG_CONFIG_PATH', 'LD_LIBRARY_PATH', 'OXPINYIN_TKRZW_PREFIX')}, path=tools)


def _run(command, *, env=None, cwd=None):
    subprocess.run([str(part) for part in command], env=env, cwd=cwd, check=True)


def _capture(command):
    try:
        done = subprocess.run([str(part) for part in command], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    except OSError:
        return SimpleNamespace(status=127, stdout=b'')
    return SimpleNamespace(status=done.returncode, stdout=done.stdout)


def main(argv=None, env=None, *, run=_run, capture=_capture):
    # Not a copy: on Windows only os.environ itself finds ImageOS under that spelling.
    env = os.environ if env is None else env
    parser = argparse.ArgumentParser(prog='python -m tools.dev.vcpkg')
    parser.add_argument('command', choices=('bootstrap', 'cache-key', 'install', 'cache-report'))
    parser.add_argument('--triplet')
    parser.add_argument('--packages', default='', help='space-separated port[features]:triplet specs')
    args = parser.parse_args(argv)
    if args.command in ('cache-key', 'install') and not args.triplet:
        parser.error(f'{args.command} requires --triplet')
    try:
        if args.command == 'bootstrap':
            destination = Path(env['RUNNER_TEMP']) / 'vcpkg'
            fetch_checkout(destination, env, run=run, cwd=ROOT)
            publish(env['GITHUB_ENV'], {'VCPKG_ROOT': destination.as_posix()})
        elif args.command == 'cache-key':
            selected = binary_cache(env, args.triplet, args.packages, capture=capture)
            Path(selected['outputs']['dir']).mkdir(parents=True, exist_ok=True)
            publish(env['GITHUB_ENV'], selected['environment'])
            publish(env['GITHUB_OUTPUT'], selected['outputs'])
            print(f"vcpkg checkout: {selected['environment']['VCPKG_ROOT']}; cache key: {selected['outputs']['key']}")
            if args.packages.split():
                print(f"vcpkg packages: {args.packages} (key component {selected['outputs']['packages']})")
        elif args.command == 'install':
            result = runner_install(env, args.triplet, args.packages, run=run)
            publish(env['GITHUB_ENV'], result['environment'])
            with open(env['GITHUB_PATH'], 'a', encoding='utf-8') as stream:
                stream.writelines(f'{directory}\n' for directory in result['path'])
        else:
            print(f"vcpkg cache: requested={env['REQUESTED_KEY']} restored={env.get('MATCHED_KEY', '')} "
                  f"image={env['ImageOS']}/{env['ImageVersion']}")
    except KeyError as error:
        print(f'::error::runner variable {error.args[0]} is unset', file=sys.stderr)
        return 1
    except (ValueError, OSError) as error:
        print(f'::error::{error}', file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as error:
        print(f'::error::{error}', file=sys.stderr)
        return error.returncode or 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
