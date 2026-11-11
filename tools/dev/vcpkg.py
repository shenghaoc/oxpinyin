"""vcpkg for the CI jobs: the binary cache key, the overlay install and the restore report.

`cache-key` finds the runner's vcpkg checkout, names its revision and publishes
the binary cache location and key. `install` installs the requested packages
with the repository overlay and publishes where pkg-config and PATH find them.
`cache-report` prints what the cache restore matched. A workflow keeps only the
cache restore and save actions between these commands.

Standard library only, beside the shared process service.
"""

if __name__ == '__main__':
    from .entry import require_normal_imports

    require_normal_imports()

import argparse
import hashlib
import os
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]


def executable(root):
    """The vcpkg program of the checkout at ROOT, or None when it has none."""
    for name in ('vcpkg.exe', 'vcpkg'):
        if (root / name).is_file() and os.access(root / name, os.X_OK):
            return root / name
    return None


def runner_checkout(env, *, which=shutil.which):
    """The runner image's vcpkg checkout: its named root, or the directory of the `vcpkg` on PATH."""
    named = env.get('VCPKG_INSTALLATION_ROOT')
    if named:
        root = Path(named)
    else:
        found = which('vcpkg', path=env.get('PATH'))
        if not found:
            raise ValueError('no vcpkg checkout: VCPKG_INSTALLATION_ROOT is unset and vcpkg is not on PATH')
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


def binary_cache(env, triplet, packages='', *, capture, which=shutil.which):
    """The environment and step outputs that select the binary cache for TRIPLET.

    The key changes with the checkout revision and the runner image. A macOS
    key also names the checkout path: the installed libtkrzw records its own
    path there as its install name, so an archive built under another root is
    not reused. PACKAGES,
    the space-separated specs a job installs, adds a `packages` output for the
    workflow to append: a different request is a different cache entry.
    Paths are published with forward slashes; the Windows steps run in bash.
    """
    for name in ('ImageOS', 'ImageVersion', 'RUNNER_TEMP'):
        if not env.get(name):
            raise ValueError(f'runner variable {name} is unset')
    root = runner_checkout(env, which=which)
    directory = (Path(env['RUNNER_TEMP']) / 'vcpkg-binary-cache').as_posix()
    prefix = f"vcpkg-bin-{env['ImageOS']}-{triplet}-"
    if 'osx' in triplet:
        prefix += hashlib.sha256(root.as_posix().encode()).hexdigest()[:16] + '-'
    outputs = {'dir': directory, 'prefix': prefix,
               'key': f"{prefix}{revision(root, capture=capture)}-{env['ImageOS']}-{env['ImageVersion']}"}
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
    options = [f'--overlay-ports={root / "vcpkg-overlays"}']
    if (root / 'triplets').is_dir():
        options.append(f'--overlay-triplets={root / "triplets"}')
    run([program, 'install', *(packages.split() or [f'tkrzw:{triplet}']), *options], env=env, cwd=root)
    installed = vcpkg / 'installed' / triplet
    own = (installed / 'lib/pkgconfig').as_posix()
    if 'windows' in triplet:
        # vcpkg's own pkgconf answers for the installed tree; the DLLs load from its bin.
        pkg_config = installed / 'tools/pkgconf/pkgconf.exe'
        run([pkg_config, '--modversion', '--cflags', '--libs', 'glib-2.0', 'tkrzw'],
            env=dict(env, PKG_CONFIG_PATH=own), cwd=root)
        return dict(environment={'PKG_CONFIG': pkg_config.as_posix(), 'PKG_CONFIG_PATH': own},
                    path=[(installed / 'bin').as_posix()])
    if 'osx' in triplet:
        # The installed library must answer pkg-config by itself and name its own path.
        run(['pkg-config', '--modversion', '--cflags', '--libs', 'tkrzw'], env=dict(env, PKG_CONFIG_PATH=own), cwd=root)
        run(['otool', '-D', installed / 'lib/libtkrzw.dylib'], env=env, cwd=root)
        inherited = os.pathsep + env['PKG_CONFIG_PATH'] if env.get('PKG_CONFIG_PATH') else ''
        return dict(environment={'PKG_CONFIG_PATH': own + inherited},
                    path=[(installed / 'tools/tkrzw/bin').as_posix()])
    raise ValueError(f'no install rule for triplet {triplet}')


def main(argv=None, env=None):
    from . import process
    from .runners import Finished
    # Not a copy: on Windows only os.environ itself finds ImageOS under that spelling.
    env = os.environ if env is None else env
    parser = argparse.ArgumentParser(prog='python -m tools.dev.vcpkg')
    parser.add_argument('command', choices=('cache-key', 'install', 'cache-report'))
    parser.add_argument('--triplet')
    parser.add_argument('--packages', default='', help='space-separated port[features]:triplet specs')
    args = parser.parse_args(argv)
    if args.command != 'cache-report' and not args.triplet:
        parser.error(f'{args.command} requires --triplet')
    try:
        if args.command == 'cache-key':
            selected = binary_cache(env, args.triplet, args.packages,
                                    capture=lambda command: process.capture(command, check=False, stderr=False))
            Path(selected['outputs']['dir']).mkdir(parents=True, exist_ok=True)
            publish(env['GITHUB_ENV'], selected['environment'])
            publish(env['GITHUB_OUTPUT'], selected['outputs'])
            print(f"vcpkg checkout: {selected['environment']['VCPKG_ROOT']}; cache key: {selected['outputs']['key']}")
            if args.packages.split():
                print(f"vcpkg packages: {args.packages} (key component {selected['outputs']['packages']})")
        elif args.command == 'install':
            result = runner_install(env, args.triplet, args.packages, run=process.run)
            publish(env['GITHUB_ENV'], result['environment'])
            with open(env['GITHUB_PATH'], 'a', encoding='utf-8') as stream:
                stream.writelines(f'{directory}\n' for directory in result['path'])
        else:
            print(f"vcpkg cache: requested={env['REQUESTED_KEY']} restored={env.get('MATCHED_KEY', '')} "
                  f"image={env['ImageOS']}/{env['ImageVersion']}")
    except (ValueError, KeyError) as error:
        print(f'::error::{error}', file=sys.stderr)
        return 1
    except Finished as finished:
        return finished.status
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
