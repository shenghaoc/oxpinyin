"""Build the pinned tkrzw as a static library on Windows and install it into a prefix.

Nothing packages tkrzw for Windows, so the repository builds it (decision
2026-11-21): upstream's VCMakefile through nmake, the library target alone,
with the DLL C runtime (/MD) in place of upstream's static one (/MT). The Rust
MSVC targets and the cxx bridge use the DLL C runtime; a /MT archive beside
them makes link.exe report LNK4098, which the denied warnings turn into a
failed build. Every other flag is upstream's: the override is the makefile's
own CLFLAGS text with that one substitution, passed as an nmake command-line
macro, and VCPATH and SDKPATH name the developer environment's own tools.

The install is PREFIX/include, PREFIX/lib/tkrzw.lib and
PREFIX/lib/pkgconfig/tkrzw.pc. The build script finds it through pkg-config
(PKG_CONFIG_PATH), as on every platform, and links the archive into each
binary, so no directory goes on PATH. PREFIX/REV is written last: a prefix
that carries the pinned revision holds a complete install and is reused.

The pin is tools/tkrzw/pin.json. The default prefix is the cache root's
tkrzw-<revision> entry (docs/runbooks/setup.md#cache-directory).

Standard library only, on the managed CPython: `just tkrzw`, the Windows
recipe over `python -m tools.tkrzw.windows`.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


if not __package__:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools import cachedir
from tools.dev.check import lookup, msvc_environment

PIN = Path(__file__).resolve().with_name('pin.json')
TKRZW_URL = 'https://github.com/estraier/tkrzw'
# Written last, so a prefix carrying it holds a complete build.
MARKER = 'REV'


class RecipeError(RuntimeError):
    """The fetch, the build or its install was refused."""


def pin(path=PIN):
    """The shared tkrzw pin: its version and source revision."""
    record = json.loads(Path(path).read_text(encoding='utf-8'))
    return {'version': record['version'], 'revision': record['revision']}


def default_prefix(env, revision):
    """The cache root's entry for REVISION, shared by every worktree."""
    return cachedir.root(env) / f'tkrzw-{revision[:12]}'


def built(prefix, revision):
    """Whether PREFIX holds a complete build of REVISION."""
    try:
        return (Path(prefix) / MARKER).read_text().rstrip('\n') == revision
    except OSError:
        return False


def environment(prefix, env):
    """ENV with PREFIX's pkg-config directory first on PKG_CONFIG_PATH."""
    pkgconfig = str(Path(prefix) / 'lib' / 'pkgconfig')
    child = dict(env)
    child['PKG_CONFIG_PATH'] = pkgconfig + (os.pathsep + env['PKG_CONFIG_PATH'] if env.get('PKG_CONFIG_PATH') else '')
    return child


def run(command, *, cwd, env, capture=False):
    print('+ ' + ' '.join(map(str, command)), flush=True)
    # Windows searches the parent's PATH, not ENV's, for the program; nmake
    # and git are on the developer environment's PATH.
    program = shutil.which(str(command[0]), path=lookup(env, 'PATH')) or str(command[0])
    done = subprocess.run([program, *map(str, command[1:])], cwd=cwd, env=env,
                          stdout=subprocess.PIPE if capture else None, text=True)
    if done.returncode:
        raise RecipeError(f'{command[0]} exited with status {done.returncode}')
    return done.stdout


def fetch(source, revision, env):
    """Check out REVISION into SOURCE and refuse any other commit."""
    run(['git', 'init', '-q', '.'], cwd=source, env=env)
    for delay in (2, 4, 8, None):
        try:
            run(['git', 'fetch', '-q', '--depth=1', TKRZW_URL, revision], cwd=source, env=env)
            break
        except RecipeError:
            if delay is None:
                raise
            print(f'tkrzw: retrying the fetch in {delay}s', file=sys.stderr, flush=True)
            time.sleep(delay)
    run(['git', '-c', 'advice.detachedHead=false', 'checkout', '-q', 'FETCH_HEAD'], cwd=source, env=env)
    head = run(['git', 'rev-parse', 'HEAD'], cwd=source, env=env, capture=True).strip()
    if head != revision:
        raise RecipeError(f'the fetched tkrzw is {head}, not {revision}; refusing to build it')


def macro(makefile, name):
    """The value of macro NAME in MAKEFILE's text, continuation lines joined."""
    joined = re.sub(r'[ \t]*\\\r?\n[ \t]*', ' ', makefile)
    match = re.search(rf'^{name}[ \t]*=[ \t]*(.*)$', joined, re.M)
    if not match:
        raise RecipeError(f'VCMakefile defines no {name}')
    return match[1].strip()


def library_flags(makefile):
    """Upstream's CLFLAGS with the DLL C runtime instead of the static one."""
    flags = macro(makefile, 'CLFLAGS')
    if len(re.findall(r'(?<!\S)/MT(?!\S)', flags)) != 1:
        raise RecipeError('VCMakefile carries not exactly one /MT flag; review the recipe against this revision')
    return re.sub(r'(?<!\S)/MT(?!\S)', '/MD', flags)


def overrides(makefile, env):
    """The nmake command-line macros: the C runtime, and the developer environment's own tools."""
    sdk = Path(lookup(env, 'WindowsSdkDir')) / 'Lib' / lookup(env, 'WindowsSDKLibVersion').rstrip('\\')
    tools = lookup(env, 'VCToolsInstallDir').rstrip('\\')
    return [f'CLFLAGS={library_flags(makefile)}', f'VCPATH={tools}', f'SDKPATH={sdk}']


def pc_text(prefix, version):
    """VCMakefile installs no tkrzw.pc; this one names the static tkrzw.lib.

    A prefix with spaces must reach the consumer as one escaped argument per
    flag, and pkgconf releases disagree on how a field is tokenized. A path
    written in the field itself, quoted or not, is escaped twice by pkgconf 3
    (an escaped backslash before the escaped space); a quoted intermediate
    variable loses its quotes on pkgconf 1.8 and 2 and the path splits.
    Quoting the expansion of a variable that holds the bare flag escapes once
    on 1.8.1, 2.5.1 and 3.0.3.
    """
    return (f'prefix={Path(prefix).as_posix()}\n'
            'libdir=${prefix}/lib\nincludedir=${prefix}/include\n'
            'tkrzw_libdir=-L${libdir}\ntkrzw_includedir=-I${includedir}\n\n'
            f'Name: Tkrzw\nDescription: Thread-safe DBM library\nVersion: {version}\n'
            'Libs: "${tkrzw_libdir}" -ltkrzw\nCflags: "${tkrzw_includedir}"\n')


def install(source, prefix, makefile, version):
    """Copy the public headers and the archive into PREFIX and write its tkrzw.pc."""
    prefix = Path(prefix)
    (prefix / 'include').mkdir(parents=True, exist_ok=True)
    for header in macro(makefile, 'HEADERFILES').split():
        shutil.copy2(source / header, prefix / 'include' / header)
    (prefix / 'lib' / 'pkgconfig').mkdir(parents=True, exist_ok=True)
    shutil.copy2(source / 'tkrzw.lib', prefix / 'lib' / 'tkrzw.lib')
    (prefix / 'lib' / 'pkgconfig' / 'tkrzw.pc').write_text(pc_text(prefix, version), encoding='utf-8')


def build(prefix, env):
    """Fetch the pin, build tkrzw.lib with nmake and install it into PREFIX.

    The archive is an x64 one, so the MSVC environment must target x64 to
    build or to reuse it. One builder holds the prefix at a time; a second
    is refused as busy.
    """
    pinned = pin()
    prefix = Path(prefix).resolve()
    env = msvc_environment(dict(env))
    target = lookup(env, 'VSCMD_ARG_TGT_ARCH')
    if target != 'x64':
        raise RecipeError(f'the MSVC environment targets {target or "<unset>"}, not x64; tkrzw.lib is an x64 archive')
    with cachedir.exclusive_fetch(prefix):
        if built(prefix, pinned['revision']):
            print(f'tkrzw: {pinned["revision"][:7]} is installed in {prefix}', flush=True)
            return prefix
        if prefix.exists() and any(prefix.iterdir()):
            raise RecipeError(f'{prefix} is not empty and holds no complete build; remove it')
        with tempfile.TemporaryDirectory(prefix='tkrzw-src-') as scratch:
            tree = Path(scratch)
            fetch(tree, pinned['revision'], env)
            makefile = (tree / 'VCMakefile').read_text(encoding='utf-8')
            if macro(makefile, 'PKG_VERSION') != pinned['version']:
                raise RecipeError(f'VCMakefile names version {macro(makefile, "PKG_VERSION")}, not the pinned {pinned["version"]}')
            # The library target alone: no utilities, no tests, no DLL.
            run(['nmake', '/nologo', '-f', 'VCMakefile', *overrides(makefile, env), 'tkrzw.lib'], cwd=tree, env=env)
            install(tree, prefix, makefile, pinned['version'])
        (prefix / MARKER).write_text(pinned['revision'] + '\n')
    print(f'tkrzw: {pinned["revision"][:7]} installed in {prefix}', flush=True)
    return prefix


def main(argv=None):
    env = dict(os.environ)
    parser = argparse.ArgumentParser(prog='python -m tools.tkrzw.windows', description=__doc__.splitlines()[0])
    parser.add_argument('--prefix', type=Path, help='install here instead of the cache root entry')
    parser.add_argument('--github-env', action='store_true', help='also append PKG_CONFIG_PATH to $GITHUB_ENV')
    args = parser.parse_args(argv)
    try:
        prefix = args.prefix or default_prefix(env, pin()['revision'])
        prefix = build(prefix, env)
    except (RecipeError, ValueError, OSError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1
    selected = environment(prefix, env)['PKG_CONFIG_PATH']
    if args.github_env:
        with open(env['GITHUB_ENV'], 'a', encoding='utf-8') as stream:
            stream.write(f'PKG_CONFIG_PATH={selected}\n')
    print(f'set PKG_CONFIG_PATH={selected}', flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
