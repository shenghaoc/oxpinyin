"""Small correctness checks for CI, and the Visual Studio environment lookup that the Windows builds use.

The Windows tkrzw build (tools/tkrzw/windows.py) and the measurement builds import `lookup` and
`msvc_environment`."""

import os
from pathlib import Path
import platform
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]

# The request-literal gate deliberately permits only the repository's one
# managed-Python declaration.
REQUEST = re.compile(r'cpython@\d|uv python (?:install|find|pin)[^\n]*? ["\']?\d\.\d|--python[ =]["\']?\d\.\d'
                     r'|requires-python\s*=\s*["\'][^"\'\n]*\d')
DECLARATION = re.compile(r'requires-python = ">=\d+\.\d+"')
DECLARED_IN = ('pyproject.toml', 'uv.lock')


def lint_python_request():
    """Check that the managed-Python version is requested only once."""
    if not re.fullmatch(r'\d+\.\d+', (ROOT / '.python-version').read_text().strip()):
        print('invalid series in .python-version', file=sys.stderr)
        return 1
    paths = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    status = 0
    for name in filter(None, paths):
        path = ROOT / name
        if name == '.python-version' or name.startswith('docs/findings/') or path.is_symlink() or not path.is_file():
            continue
        try:
            lines = path.read_text().splitlines()
        except UnicodeDecodeError:
            continue
        matches = [line for line in lines if REQUEST.search(line)]
        if name in DECLARED_IN:
            declared = [line for line in matches if DECLARATION.fullmatch(line)]
            if len(declared) == 1:
                matches.remove(declared[0])
            elif name == 'pyproject.toml':
                print(f'{name}: expected one minimum-version declaration', file=sys.stderr)
                status = 1
        for line in matches:
            print(f'{name}: unexpected Python version request: {line}', file=sys.stderr)
            status = 1
    return status


def lookup(env, name):
    """Return NAME's value in ENV, ignoring case for Windows environments."""
    return next((value for key, value in env.items() if key.upper() == name.upper()), '')


def msvc_environment(env):
    """Return the caller's VS environment, or load one through vswhere."""
    if lookup(env, 'VCToolsInstallDir'):
        return env
    root = lookup(env, 'ProgramFiles(x86)') or r'C:\Program Files (x86)'
    vswhere = Path(root) / 'Microsoft Visual Studio' / 'Installer' / 'vswhere.exe'
    found = subprocess.run([str(vswhere), '-latest', '-products', '*', '-requires',
                            'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'],
                           env=env, capture_output=True, text=True)
    installation = found.stdout.strip().splitlines()[0] if found.returncode == 0 and found.stdout.strip() else ''
    if not installation:
        raise ValueError('no Visual Studio with the C++ tools found; run from a Developer PowerShell for VS')
    script = 'vcvarsarm64.bat' if platform.machine().upper() == 'ARM64' else 'vcvars64.bat'
    vcvars = Path(installation) / 'VC' / 'Auxiliary' / 'Build' / script
    done = subprocess.run(f'cmd /d /s /c ""{vcvars}" >nul && set"', env=env, capture_output=True, text=True)
    if done.returncode:
        raise ValueError(f'{vcvars} failed with status {done.returncode}')
    loaded = dict(line.split('=', 1) for line in done.stdout.splitlines()
                  if '=' in line and not line.startswith('='))
    if not lookup(loaded, 'VCToolsInstallDir'):
        raise ValueError(f'{vcvars} did not set VCToolsInstallDir')
    return loaded


LINT_USAGE = ('usage: python -c "from tools.dev.check import lint_crate_cfg_placement; import sys; '
              'sys.exit(lint_crate_cfg_placement(sys.argv[1:]))" [file...]\n'
              '       with no arguments, lints every tracked *.rs file\n')
LINT_VIOLATION_HEADER = ('crate-cfg-placement: a crate-level `#![cfg(...)]` must sit BELOW '
                         'its `//!` docs.\n\n')
LINT_VIOLATION_FOOTER = (
    'A false crate-level `#![cfg(...)]` discards the crate attributes that follow\n'
    'it, so these docs vanish on the excluded platform and `missing_docs` fires\n'
    'there. Move the `#![cfg(...)]` below the `//!` block; the gate keeps its\n'
    'predicate and its whole-crate scope.\n')


def _placement_hit(path):
    """Return (gate line, doc line) for the first crate-level docs after a gate."""
    gate = 0
    with path.open('rb') as source:
        for number, line in enumerate(source, 1):
            if gate == 0 and line.startswith(b'#![cfg('):
                gate = number
            elif gate and line.startswith(b'//!'):
                return gate, number
    return None


def lint_crate_cfg_placement(argv):
    """Lint crate-level cfg placement and return 0, 1 or 2."""
    out, err = sys.stdout, sys.stderr
    args = list(argv)
    if args[:1] in (['-h'], ['--help']):
        err.write(LINT_USAGE)
        return 2
    if args:
        unreadable = [f'  {name}\n' for name in args if not Path(name).is_file()]
        if unreadable:
            err.write('crate-cfg-placement: not a readable regular file:\n' + ''.join(unreadable))
            return 2
        files = args
    else:
        try:
            result = subprocess.run(['git', 'ls-files', '--', '*.rs'], cwd=ROOT,
                                    capture_output=True, check=False)
        except OSError:
            result = None
        if result is None or result.returncode:
            err.write('crate-cfg-placement: not a git repository and no files given\n')
            return 2
        files = [os.fsdecode(name) for name in result.stdout.split(b'\n') if name]
    violations = []
    checked = 0
    named = bool(args)
    for name in files:
        path = Path(name) if named else ROOT / name
        if not path.is_file():
            continue
        checked += 1
        hit = _placement_hit(path)
        if hit:
            gate, doc = hit
            violations.append(f'  {name}:{doc}: `//!` doc comment below the crate-level '
                              f'`#![cfg(...)]` on line {gate}\n')
    if violations:
        err.write(LINT_VIOLATION_HEADER)
        err.write(''.join(violations))
        err.write('\n')
        err.write(LINT_VIOLATION_FOOTER)
        return 1
    print(f'crate-cfg-placement: {checked} file(s) checked, no violations', file=out)
    return 0
