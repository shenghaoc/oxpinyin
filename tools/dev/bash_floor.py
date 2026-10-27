"""The one Bash floor declaration and its source-line gate.

Every executed Bash script sources tools/lib/bash-floor.sh as its first
command. Source-only libraries do not: their caller has already run the
check. This module reads the declared floor for doctor and the lint job,
and rejects a script that carries an inline version guard instead.
"""
import posixpath
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
FLOOR_FILE = 'tools/lib/bash-floor.sh'
FLOOR_RE = re.compile(r'^OXPINYIN_BASH_FLOOR=([0-9]+(?:\.[0-9]+)*)[ \t]*$', re.M)
INLINE_GUARD = re.compile(r'\b(?:BASH_VERSION|BASH_VERSINFO)\b')
VERSION_LITERAL = re.compile(r'(?i)\bbash\b[ \t]*(?:>=|>|at[ \t]+least)[ \t]*[0-9]')

# Files that are sourced, never executed. Their caller has run the floor
# check, so they neither source the guard nor carry a version themselves.
SOURCED_ONLY = frozenset({
    'tools/cloud/env.sh',
    'tools/container/runtime.sh',
    'tools/oracle/pin.sh',
})

# Scripts that are both executed and sourced. They must not re-run the
# relative-path check when a caller has changed directory, so the source
# line runs only for direct execution.
MIXED = frozenset({
    'tools/packaging/function-order.sh',
    'tools/tkrzw/runtime.sh',
})

# Scripts that run only from a staged copy, with the floor file staged beside
# them: compat.sh runs compat-inside.sh as /input/inside.sh.
STAGED = frozenset({
    'tools/oracle/compat-inside.sh',
})


def read_floor():
    """Return the floor declared in FLOOR_FILE."""
    match = FLOOR_RE.search((ROOT / FLOOR_FILE).read_text(encoding='utf-8'))
    if match is None:
        raise ValueError(f'{FLOOR_FILE}: no OXPINYIN_BASH_FLOOR assignment')
    return match.group(1)


def source_line(relative):
    """Return the POSIX source line a script at RELATIVE must use."""
    target = (posixpath.basename(FLOOR_FILE) if relative in STAGED
              else posixpath.relpath(FLOOR_FILE, posixpath.dirname(relative)))
    if relative in MIXED:
        return f'[ "${{BASH_SOURCE:-$0}}" = "$0" ] && . "${{0%/*}}/{target}"'
    return f'. "${{0%/*}}/{target}"'


def tracked_shell_scripts():
    """Return tracked *.sh paths relative to ROOT in git's order."""
    output = subprocess.run(['git', 'ls-files', '-z', '--', '*.sh'], cwd=ROOT,
                            check=True, capture_output=True).stdout
    return [path.decode() for path in output.split(b'\0') if path]


def first_command(lines):
    """Return the first executable line after the shebang and comments."""
    for index, line in enumerate(lines):
        if index == 0 and line.startswith('#!'):
            continue
        stripped = line.strip()
        if not stripped or stripped.startswith('#'):
            continue
        return line.rstrip()
    return None


def _code(line):
    """Return the code part of LINE, without a trailing comment."""
    return line.split('#', 1)[0]


def check_text(relative, text):
    """Return the floor-gate errors for one shell file."""
    if relative == FLOOR_FILE:
        return []
    lines = text.splitlines()
    first = first_command(lines)
    expected = source_line(relative)
    errors = []
    if relative in SOURCED_ONLY:
        for number, line in enumerate(lines, 1):
            if INLINE_GUARD.search(_code(line)) or VERSION_LITERAL.search(_code(line)):
                errors.append(f'{relative}:{number}: source-only library carries a Bash version guard or literal')
        return errors
    if first != expected:
        errors.append(f'{relative}: first command must be {expected!r}, found {first!r}')
    for number, line in enumerate(lines, 1):
        code = _code(line)
        if INLINE_GUARD.search(code):
            errors.append(f'{relative}:{number}: inline Bash version guard')
        elif VERSION_LITERAL.search(code):
            errors.append(f'{relative}:{number}: inline Bash version literal')
    return errors


def check_repository():
    """Return every floor-gate error in the tracked shell tree."""
    errors = []
    for relative in tracked_shell_scripts():
        text = (ROOT / relative).read_text(encoding='utf-8')
        first = text.splitlines()[0] if text.splitlines() else ''
        if first.startswith('#!') and 'bash' not in first:
            continue
        errors.extend(check_text(relative, text))
    return errors


def main():
    """Run the floor gate over the repository."""
    errors = check_repository()
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        print('::error::the Bash floor gate failed', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
