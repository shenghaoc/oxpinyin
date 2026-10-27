"""The host loader resolver shared by native, measurement and compat callers.

Backend labels are advisory: certify the complete set the actual loader resolves,
against the explicitly configured healthy library. No caller infers loader trust.
"""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


if not __package__:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))


class RuntimeRefused(RuntimeError):
    """The object's loader cannot be certified against the configured runtime."""


def check(cell, runner, side, obj, *, env=None, cwd=None):
    """Return the loaded libtkrzw path, None if absent, or raise RuntimeRefused.

    Inspect OBJ in the caller's environment and working directory. CELL is an
    advisory label, preserved for callers; it never bypasses qualification.
    """
    environment = dict(os.environ if env is None else env)
    directory = Path.cwd() if cwd is None else Path(cwd).resolve()
    obj = str(obj)
    side = Path(side).name
    side = 'pin' if re.match(r'^(?:L$|A$|pin|oracle|reference|libpinyin)', side) else 'subject'

    def refuse(found, expected):
        raise RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to {found}; expected {expected} '
                             f'(object {obj}, cwd {directory})')

    try:
        host = subprocess.run(['uname', '-s'], env=environment, cwd=directory,
                              capture_output=True, text=True, timeout=30).stdout.strip()
        if host == 'Darwin' and any(environment.get(name) for name in
                                    ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH')):
            refuse('<DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH set>',
                   '<otool -L recorded install name; a dyld override can bypass it>')
        done = subprocess.run(['otool', '-L', obj] if host == 'Darwin' else ['ldd', obj],
                              env=environment, cwd=directory, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to <unknown>: {error}') from error
    matches = []
    for line in done.stdout.splitlines():
        if host == 'Darwin':
            match = re.match(r'\s*(/.*libtkrzw\S*\.dylib)\s+\(compatibility version', line)
            if match:
                matches.append(match[1])
        else:
            match = re.match(r'\s*libtkrzw\.so\S*\s+=>\s*(.*)', line)
            if match:
                matches.append(re.sub(r'\s+\(0x[0-9a-fA-F]+\)\s*$', '', match[1]))
            elif re.match(r'\s*(/|\./|\.\./).*/libtkrzw\.so.*\s+\(0x[0-9a-fA-F]+\)', line):
                matches.append(re.sub(r'\s+\(0x[0-9a-fA-F]+\)\s*$', '', line.lstrip()))
    if not matches and done.returncode == 0:
        if 'libtkrzw' in done.stdout:
            refuse('<unresolved libtkrzw>', '<verified runtime>')
        return None
    found, resolved, invalid, ambiguous = [], None, False, False
    for entry in matches:
        path = Path(entry)
        path = path if path.is_absolute() else directory / path
        real = str(path.resolve()) if path.is_file() else entry
        invalid |= not path.is_file()
        if resolved is None:
            found.append(entry)
            resolved = real
        elif real != resolved:
            found.append(entry)
            ambiguous = True
    description = '; '.join(found) or '<missing; resolver failed>'
    expected = environment.get('OXPINYIN_TKRZW_LIBRARY', '')
    if not expected:
        prefix = environment.get('OXPINYIN_TKRZW_PREFIX') or environment.get('OXPINYIN_TKRZW_PREFIXES', '').split(':')[0]
        expected = prefix + ('/lib/libtkrzw.dylib' if host == 'Darwin' else '/lib/libtkrzw.so.1') if prefix else ''
    path = Path(expected)
    path = path if path.is_absolute() else directory / path
    if not expected or not path.is_file():
        refuse(description, expected or '<unconfigured verified runtime>')
    expected = str(path.resolve())
    if done.returncode or invalid or ambiguous or resolved != expected:
        refuse(description, expected)
    return Path(resolved)


def binary(path):
    """Whether PATH is an ELF or Mach-O object, including directly linked workers."""
    try:
        with Path(path).open('rb') as stream:
            return stream.read(4) in (b'\x7fELF', bytes.fromhex('cffaedfe'), bytes.fromhex('cefaedfe'),
                                      bytes.fromhex('feedfacf'), bytes.fromhex('feedface'))
    except OSError:
        return False


def qualify(cell, runner, side, obj, command=(), *, env=None, cwd=None):
    """Check OBJ and a directly linked command, preserving the shell wrapper rule."""
    resolved = check(cell, runner, side, obj, env=env, cwd=cwd)
    if command:
        environment = os.environ if env is None else env
        executable = shutil.which(str(command[0]), path=environment.get('PATH'))
        if executable is None:
            raise FileNotFoundError(str(command[0]))
        if executable != str(obj) and binary(executable):
            check(cell, runner, side, executable, env=env, cwd=cwd)
    return resolved


def main(argv=None):
    """Preserve the runtime.sh CLI, including child execution and signal statuses."""
    args = list(sys.argv[1:] if argv is None else argv)
    quiet = print_path = False
    while args and args[0] in ('--quiet-stderr', '--print-path'):
        quiet |= args[0] == '--quiet-stderr'
        print_path |= args.pop(0) == '--print-path'
    if len(args) < 4:
        print('usage: runtime.sh [--quiet-stderr] [--print-path] CELL RUNNER SIDE OBJECT [-- COMMAND...]', file=sys.stderr)
        return 2
    cell, runner, side, obj, *command = args
    if command[:1] == ['--']:
        command.pop(0)
    try:
        path = qualify(cell, runner, side, obj, command)
    except RuntimeRefused as error:
        print(f'REFUSED: {error}', file=sys.stderr)
        return 1
    except FileNotFoundError:
        return 1  # command -v in the legacy launcher
    if print_path and path:
        print(path, flush=True)
    if command:
        from tools.dev.native import launch_status
        if quiet:
            descriptor = os.open(os.devnull, os.O_WRONLY)
            os.dup2(descriptor, 2)
            os.close(descriptor)
        try:
            os.execvpe(command[0], command, os.environ)
        except OSError as error:
            print(f'{command[0]}: {error.strerror}', file=sys.stderr)
            return launch_status(error)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
