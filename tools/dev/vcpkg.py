"""vcpkg for the Windows CI job: GLib and pkgconf.

`cache-key` finds the runner image's checkout, names its revision and publishes
the binary cache location and key. `install` installs the requested packages
and publishes where pkg-config, the loader and PATH find them. `cache-report`
prints what the cache restore matched. A workflow keeps only the cache restore
and save actions between these commands.

Windows builds tkrzw itself (`just tkrzw`, tools/tkrzw/windows.py). Linux uses
no vcpkg: every Linux build takes Kyoto Cabinet from Ubuntu's packages
(docs/runbooks/setup.md#linux). macOS uses Homebrew's tkrzw
(docs/runbooks/setup.md#macos).
"""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def executable(root):
    """The vcpkg program of the checkout at ROOT, or None when it has none."""
    for name in ('vcpkg.exe', 'vcpkg'):
        if (root / name).is_file():
            return root / name
    return None


def runner_checkout(env):
    """A CI job's vcpkg checkout: the image's named root, an explicit VCPKG_ROOT, or the `vcpkg` on PATH."""
    named = env.get('VCPKG_INSTALLATION_ROOT') or env.get('VCPKG_ROOT')
    if named:
        root = Path(named)
    else:
        found = shutil.which('vcpkg', path=env.get('PATH'))
        if not found:
            raise ValueError('no vcpkg checkout: VCPKG_INSTALLATION_ROOT and VCPKG_ROOT are unset '
                             'and vcpkg is not on PATH')
        root = Path(os.path.realpath(found)).parent
    if executable(root) is None:
        raise ValueError(f"no vcpkg checkout at '{root}'")
    return root


def revision(root):
    """The checkout's commit, or a digest of its baseline when it carries no Git metadata."""
    try:
        done = subprocess.run(['git', '-C', str(root), 'rev-parse', 'HEAD'],
                              capture_output=True, text=True)
        if done.returncode == 0 and done.stdout.strip():
            return done.stdout.strip()
    except OSError:
        pass
    baseline = hashlib.sha256((root / 'versions/baseline.json').read_bytes()).hexdigest()[:40]
    return 'baseline-' + baseline


def binary_cache(env, triplet, packages=''):
    """The environment and step outputs that select the binary cache for TRIPLET.

    The key changes with the checkout revision and with the toolchain, which
    the hosted Windows runner image names. PACKAGES, the space-separated specs
    a job installs, adds a `packages` output for the workflow to append: a
    different request is a different cache entry.
    Paths are published with forward slashes; the Windows steps run in bash.
    """
    if 'windows' not in triplet:
        raise ValueError(f'no vcpkg cache rule for triplet {triplet}')
    for name in ('ImageOS', 'ImageVersion', 'RUNNER_TEMP'):
        if not env.get(name):
            raise ValueError(f'runner variable {name} is unset')
    root = runner_checkout(env)
    directory = (Path(env['RUNNER_TEMP']) / 'vcpkg-binary-cache').as_posix()
    name = revision(root)
    prefix = f"vcpkg-bin-{env['ImageOS']}-{triplet}-"
    key = f"{prefix}{name}-{env['ImageOS']}-{env['ImageVersion']}"
    outputs = {'dir': directory, 'prefix': prefix, 'key': key}
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


def runner_install(env, triplet, packages):
    """Install PACKAGES for TRIPLET, check GLib, and return what later steps need.

    PACKAGES is the job's space-separated port[features]:triplet specs. The
    result names the environment variables to publish and the directories to
    put on PATH.
    """
    if 'windows' not in triplet:
        raise ValueError(f'no install rule for triplet {triplet}')
    if not packages.split():
        raise ValueError('install requires --packages')
    if not env.get('VCPKG_ROOT'):
        raise ValueError('VCPKG_ROOT is unset; run cache-key first')
    vcpkg = Path(env['VCPKG_ROOT'])
    program = executable(vcpkg)
    if program is None:
        raise ValueError(f"no vcpkg checkout at '{vcpkg}'")
    subprocess.run([program, 'install', *packages.split()], env=env, cwd=ROOT, check=True)
    installed = vcpkg / 'installed' / triplet
    own = (installed / 'lib/pkgconfig').as_posix()
    # vcpkg's own pkgconf answers for the installed tree; GLib's DLLs load from its bin.
    pkg_config = installed / 'tools/pkgconf/pkgconf.exe'
    subprocess.run([pkg_config, '--modversion', '--cflags', '--libs', 'glib-2.0'],
                   env=dict(env, PKG_CONFIG_PATH=own), cwd=ROOT, check=True)
    return dict(environment={'PKG_CONFIG': pkg_config.as_posix(), 'PKG_CONFIG_PATH': own},
                path=[(installed / 'bin').as_posix()])


def main(argv=None):
    # Not a copy: on Windows only os.environ itself finds ImageOS under that spelling.
    env = os.environ
    parser = argparse.ArgumentParser(prog='python -m tools.dev.vcpkg')
    parser.add_argument('command', choices=('cache-key', 'install', 'cache-report'))
    parser.add_argument('--triplet')
    parser.add_argument('--packages', default='', help='space-separated port[features]:triplet specs')
    args = parser.parse_args(argv)
    if args.command in ('cache-key', 'install') and not args.triplet:
        parser.error(f'{args.command} requires --triplet')
    try:
        if args.command == 'cache-key':
            selected = binary_cache(env, args.triplet, args.packages)
            Path(selected['outputs']['dir']).mkdir(parents=True, exist_ok=True)
            publish(env['GITHUB_ENV'], selected['environment'])
            publish(env['GITHUB_OUTPUT'], selected['outputs'])
            print(f"vcpkg checkout: {selected['environment']['VCPKG_ROOT']}; cache key: {selected['outputs']['key']}")
            if args.packages.split():
                print(f"vcpkg packages: {args.packages} (key component {selected['outputs']['packages']})")
        elif args.command == 'install':
            result = runner_install(env, args.triplet, args.packages)
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
