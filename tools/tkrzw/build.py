"""Build upstream tkrzw at the provisioning revision into a prefix.

tools/cloud/setup.sh builds this revision when no certified host libtkrzw is
available, and CI's tkrzw composite action builds it on Linux and macOS. The source
is fetched by commit and the checkout is checked against that commit before
anything is built. Linux and macOS use
`./configure --prefix=DIR && make && make install`. Windows uses the vcpkg
overlay port, which reads the same pin. Every
build leaves DIR/lib/pkgconfig/tkrzw.pc, so a consumer finds it through
PKG_CONFIG_PATH, and must hold its headers, library, tkrzw.pc and utility (with
the utility loading DIR's own shared libtkrzw) before DIR/REV is written.
`check --prefix` requires an existing prefix to carry DIR/REV and pass the same
install check.

A build from this recipe needs no distro-probe.sh: the defect that probe detects
is in Ubuntu's packaged libtkrzw (docs/findings/tkrzw-distro-compat.md), and this
recipe drops the build flags that expose it.

Standard library only, and no syntax newer than Python 3.9: the CI action runs
this with the runner's own Python, which some jobs have instead of the managed
interpreter.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time


if not __package__:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.tkrzw import runtime

# 1.0.34: its sentinel fix makes defect 1 immune to LTO (tkrzw-distro-compat.md).
PIN = json.loads((Path(__file__).resolve().parents[2] / 'vcpkg-overlays/tkrzw/pin.json').read_text())
TKRZW_REV = PIN['revision']
TKRZW_URL = 'https://github.com/estraier/tkrzw'
# Written last, so a prefix carrying it holds a complete, checked build.
MARKER = 'REV'
LIBRARY = {'Linux': 'lib/libtkrzw.so.1', 'Darwin': 'lib/libtkrzw.dylib'}


class RecipeError(RuntimeError):
    """The fetch, the build or its install check was refused."""


def host():
    name = platform.system()
    if name not in LIBRARY:
        raise RecipeError(f'no tkrzw recipe for {name}')
    return name


def lookup(env, name):
    """NAME's value in ENV, ignoring case: os.environ upper-cases Windows names."""
    return next((value for key, value in env.items() if key.upper() == name.upper()), '')


def run(command, *, cwd, env, capture=False):
    print('+ ' + ' '.join(map(str, command)), flush=True)
    # Resolve against the requested environment rather than the parent's PATH.
    program = shutil.which(str(command[0]), path=lookup(env, 'PATH')) or str(command[0])
    done = subprocess.run([program, *map(str, command[1:])], cwd=cwd, env=env,
                          stdout=subprocess.PIPE if capture else None, text=True)
    if done.returncode:
        raise RecipeError(f'{command[0]} exited with status {done.returncode}')
    return done.stdout


def fetch(source, env, *, runner=run, delays=(2, 4, 8, 16), sleep=time.sleep):
    """Check out TKRZW_REV into SOURCE and refuse any other commit."""
    runner(['git', 'init', '-q', '.'], cwd=source, env=env)
    for delay in (*delays, None):
        try:
            runner(['git', 'fetch', '-q', '--depth=1', TKRZW_URL, TKRZW_REV], cwd=source, env=env)
            break
        except RecipeError:
            if delay is None:
                raise
            print(f'tkrzw: retrying in {delay}s: git fetch -q --depth=1 {TKRZW_URL} {TKRZW_REV}', file=sys.stderr, flush=True)
            sleep(delay)
    runner(['git', '-c', 'advice.detachedHead=false', 'checkout', '-q', 'FETCH_HEAD'], cwd=source, env=env)
    head = runner(['git', 'rev-parse', 'HEAD'], cwd=source, env=env, capture=True).strip()
    if head != TKRZW_REV:
        raise RecipeError(f'the fetched tkrzw is {head}, not {TKRZW_REV}; refusing to build it')
    print(f'tkrzw: source verified at {TKRZW_REV}', flush=True)


def make_jobs(env, cpus=None):
    """make's -j: the caller's CARGO_BUILD_JOBS, read as
    cargo reads it ("default" is the CPU count, a negative count is taken from
    it, at least one), or the CPU count when nothing set one."""
    cpus = cpus or os.cpu_count() or 1
    setting = lookup(env, 'CARGO_BUILD_JOBS')
    if not setting or setting == 'default':
        return cpus
    if not re.fullmatch(r'[+-]?[0-9]+', setting) or int(setting) == 0:
        raise RecipeError(f'invalid CARGO_BUILD_JOBS {setting!r}: use a nonzero integer or default')
    count = int(setting)
    return max(1, cpus + count) if count < 0 else count


def posix_build(source, prefix, jobs, env):
    # Build flags from the caller's environment are dropped, so no -flto or
    # -Wl,-Bsymbolic-functions can come in (tkrzw-distro-compat.md explains both).
    env = {name: value for name, value in env.items() if name not in ('CFLAGS', 'CXXFLAGS', 'LDFLAGS')}
    run(['./configure', f'--prefix={prefix}'], cwd=source, env=env)
    run(['make', f'-j{jobs}'], cwd=source, env=env)
    run(['make', 'install'], cwd=source, env=env)


def darwin_pc(text):
    """configure writes `-lstdc++` into tkrzw.pc on every host; macOS SDKs have
    no libstdc++, and the library itself links libc++."""
    return re.sub(r'(?<=\s)-lstdc\+\+(?=\s|$)', '-lc++', text)


def msvc_environment(env, *, capture=subprocess.run):
    """The Visual Studio developer environment: the caller's, or vcvars' via vswhere."""
    if lookup(env, 'VCToolsInstallDir'):
        return env
    root = lookup(env, 'ProgramFiles(x86)') or r'C:\Program Files (x86)'
    vswhere = Path(root) / 'Microsoft Visual Studio' / 'Installer' / 'vswhere.exe'
    found = capture([str(vswhere), '-latest', '-products', '*', '-requires',
                     'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'],
                    env=env, capture_output=True, text=True)
    installation = found.stdout.strip().splitlines()[0] if found.returncode == 0 and found.stdout.strip() else ''
    if not installation:
        raise RecipeError('no Visual Studio with the C++ tools found; run from a Developer PowerShell for VS')
    script = 'vcvarsarm64.bat' if platform.machine().upper() == 'ARM64' else 'vcvars64.bat'
    vcvars = Path(installation) / 'VC' / 'Auxiliary' / 'Build' / script
    # cmd /s keeps the outer quotes literal, so the batch path may hold spaces.
    done = capture(f'cmd /d /s /c ""{vcvars}" >nul && set"', env=env, capture_output=True, text=True)
    if done.returncode:
        raise RecipeError(f'{vcvars} failed with status {done.returncode}')
    # `set` also lists cmd's hidden per-drive "=C:=C:\\..." entries; skip them.
    loaded = dict(line.split('=', 1) for line in done.stdout.splitlines()
                  if '=' in line and not line.startswith('='))
    if not lookup(loaded, 'VCToolsInstallDir'):
        raise RecipeError(f'{vcvars} did not set VCToolsInstallDir')
    return loaded


def install_environment(prefix, env, system):
    child = dict(env)
    path = next((key for key in child if key.upper() == 'PATH'), 'PATH')
    child[path] = str(Path(prefix) / 'bin') + os.pathsep + child.get(path, '')
    # otool -L must read the recorded install name, not a dyld override.
    for name in ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH'):
        child.pop(name, None)
    if system == 'Linux':
        child['LD_LIBRARY_PATH'] = str(Path(prefix) / 'lib') + (':' + env['LD_LIBRARY_PATH'] if env.get('LD_LIBRARY_PATH') else '')
    return child


def check_install(prefix, env=None, *, system=None, resolve=runtime.resolve):
    """Require PREFIX's installed files and, where libtkrzw is a shared
    library, that its utility loads PREFIX's own libtkrzw."""
    system = system or host()
    env = dict(os.environ if env is None else env)
    prefix = Path(prefix)
    util = prefix / 'bin' / 'tkrzw_dbm_util'
    missing = [str(path) for path in (prefix / 'include/tkrzw_dbm.h', prefix / LIBRARY[system],
                                      prefix / 'lib/pkgconfig/tkrzw.pc', util) if not path.is_file()]
    if missing:
        raise RecipeError(f'{prefix} lacks {", ".join(missing)}')
    child = install_environment(prefix, env, system)
    library = prefix / LIBRARY[system]
    _, done, matches = resolve(util, env=child)
    resolved = {str(Path(match).resolve()) for match in matches if Path(match).is_file()}
    if done.returncode or resolved != {str(library.resolve())}:
        raise RecipeError(f'{util} resolves libtkrzw to {sorted(resolved) or "nothing"}, not {library}')
    print(f'tkrzw: {util.name} loads {library}', flush=True)


def built(prefix):
    """Whether PREFIX holds a complete, checked build of TKRZW_REV."""
    try:
        return (Path(prefix) / MARKER).read_text().rstrip('\n') == TKRZW_REV
    except OSError:
        return False


def check(prefix):
    """Require PREFIX to hold TKRZW_REV's complete, checked install."""
    if not built(prefix):
        raise RecipeError(f'{prefix} does not hold a checked build of {TKRZW_REV}')
    check_install(prefix)


def build(prefix, *, source=None, jobs=None, env=None):
    """Fetch, verify, build, install and check TKRZW_REV into an empty PREFIX."""
    system = host()
    env = dict(os.environ if env is None else env)
    prefix = Path(prefix).resolve()
    jobs = jobs or make_jobs(env)
    if prefix.exists() and any(prefix.iterdir()):
        raise RecipeError(f'{prefix} is not empty; remove it or choose another prefix')
    with tempfile.TemporaryDirectory(prefix='tkrzw-src-') as scratch:
        tree = Path(source).resolve() if source else Path(scratch)
        tree.mkdir(parents=True, exist_ok=True)
        fetch(tree, env)
        prefix.mkdir(parents=True, exist_ok=True)
        posix_build(tree, prefix, jobs, env)
    if system == 'Darwin':
        pc = prefix / 'lib/pkgconfig/tkrzw.pc'
        pc.write_text(darwin_pc(pc.read_text()))
    check_install(prefix, env, system=system)
    (prefix / MARKER).write_text(TKRZW_REV + '\n')
    print(f'tkrzw: {TKRZW_REV[:7]} installed in {prefix}; set PKG_CONFIG_PATH={prefix / "lib" / "pkgconfig"} '
          f'and the runtime library path to use it', flush=True)
    return prefix


def main(argv=None):
    parser = argparse.ArgumentParser(prog='tools.tkrzw.build', description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('rev', help='print the revision this recipe builds')
    check_command = commands.add_parser('check', help='require a restored prefix to hold this revision and its install')
    check_command.add_argument('--prefix', required=True, type=Path)
    command = commands.add_parser('build', help='fetch, verify, build, install and check into an empty prefix')
    command.add_argument('--prefix', required=True, type=Path)
    command.add_argument('--source', type=Path, help='check the source out here instead of a temporary directory')
    command.add_argument('--jobs', type=int)
    args = parser.parse_args(argv)
    try:
        if args.command == 'rev':
            print(TKRZW_REV)
        elif args.command == 'check':
            check(args.prefix)
        else:
            build(args.prefix, source=args.source, jobs=args.jobs)
    except RecipeError as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
