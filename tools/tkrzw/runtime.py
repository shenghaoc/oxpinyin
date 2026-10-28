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


class LoaderOverride(RuntimeError):
    """A dyld override prevents install-name resolution from certifying a library."""


class LoaderFailure(RuntimeError):
    """The host loader command could not be executed or timed out."""

    def __init__(self, message, status=1):
        super().__init__(message)
        self.status = status


def resolve(obj, *, env=None, cwd=None, probe=False):
    """Read loader dependencies once for certification or pre-certification host probes.

    PROBE preserves env.sh's historical libtkrzw.so.1/first-install-name view;
    certification always checks the full resolved dependency set.
    """
    environment = dict(os.environ if env is None else env)
    directory = Path.cwd() if cwd is None else Path(cwd).resolve()
    obj = str(obj)
    try:
        host = subprocess.run(['uname', '-s'], env=environment, cwd=directory,
                              capture_output=True, text=True, timeout=30).stdout.strip()
        if host == 'Darwin' and any(environment.get(name) for name in
                                    ('DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH')):
            raise LoaderOverride()
        done = subprocess.run(['otool', '-L', obj] if host == 'Darwin' else ['ldd', obj],
                              env=environment, cwd=directory, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, text=True, timeout=30)
    except OSError as error:
        from tools.dev.native import launch_status
        raise LoaderFailure(str(error), launch_status(error)) from error
    except subprocess.TimeoutExpired as error:
        raise LoaderFailure(str(error)) from error
    matches = []
    for line in done.stdout.splitlines():
        if host == 'Darwin':
            match = re.match(r'\s*(/.*libtkrzw\S*\.dylib)\s+\(compatibility version', line)
            if match:
                matches.append(match[1])
        else:
            match = re.match(r'\s*(libtkrzw\.so\S*)\s+=>\s*(.*)', line)
            if match:
                entry = re.sub(r'\s+\(0x[0-9a-fA-F]+\)\s*$', '', match[2])
                if not probe or match[1] == 'libtkrzw.so.1' and entry.startswith('/'):
                    matches.append(entry)
            elif not probe and re.match(r'\s*(/|\./|\.\./).*/libtkrzw\.so.*\s+\(0x[0-9a-fA-F]+\)', line):
                matches.append(re.sub(r'\s+\(0x[0-9a-fA-F]+\)\s*$', '', line.lstrip()))
    return host, done, matches


def check(cell, runner, side, obj, *, env=None, cwd=None):
    """Return the loaded libtkrzw path, None if absent, or raise RuntimeRefused.

    Inspect OBJ in the caller's environment and working directory. CELL is an
    advisory label, preserved for callers; it never bypasses qualification.
    """
    environment = dict(os.environ if env is None else env)
    directory = Path.cwd() if cwd is None else Path(cwd).resolve()
    obj = str(obj)
    side = str(side).rsplit('/', 1)[-1]
    side = 'pin' if re.match(r'^(?:L$|A$|pin|oracle|reference|libpinyin)', side) else 'subject'

    def refuse(found, expected):
        raise RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to {found}; expected {expected} '
                             f'(object {obj}, cwd {directory})')
    try:
        host, done, matches = resolve(obj, env=environment, cwd=directory)
    except LoaderOverride:
        refuse('<DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH set>',
               '<otool -L recorded install name; a dyld override can bypass it>')
    except LoaderFailure as error:
        raise RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to <unknown>: {error}') from error
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


def launched(executable, cwd=None):
    """The file the kernel loads for EXECUTABLE: itself, or a #! script's interpreter.

    ldd cannot read a script, so a wrapper such as Debian's valgrind is checked
    through its interpreter. What the script execs is not visible here; its
    subject is checked explicitly by the caller. Every other file is checked
    as itself. None means there is no file to launch.
    """
    path = Path(executable)
    for _ in range(8):  # beyond the kernel's own interpreter nesting limit
        try:
            with path.open('rb') as stream:
                line = stream.readline(256)
        except OSError:
            return path if path.is_file() else None
        words = line[2:].split() if line.startswith(b'#!') else []
        if not words:
            return path
        path = Path(os.fsdecode(words[0]))
        if not path.is_absolute():
            path = (Path.cwd() if cwd is None else Path(cwd)) / path
    return path  # a chain no kernel follows; its check refuses the script


def qualify(cell, runner, side, obj, command=(), *, env=None, cwd=None):
    """Check OBJ and the file a command launches, a script through its interpreter."""
    resolved = check(cell, runner, side, obj, env=env, cwd=cwd)
    if command:
        environment = os.environ if env is None else env
        executable = shutil.which(str(command[0]), path=environment.get('PATH'))
        if executable is None:
            raise FileNotFoundError(str(command[0]))
        started = launched(executable, cwd)
        if started is not None and str(started) != str(obj):
            check(cell, runner, side, started, env=env, cwd=cwd)
    return resolved


def main(argv=None):
    """Preserve the runtime.sh CLI, including child execution and signal statuses."""
    args = list(sys.argv[1:] if argv is None else argv)
    if args[:1] == ['--resolve']:
        if len(args) != 2:
            print('usage: runtime.py --resolve OBJECT', file=sys.stderr)
            return 2
        try:
            host, done, matches = resolve(args[1], probe=True)
            for path in matches[:1] if host == 'Darwin' else matches:
                print(path)
            return 128 - done.returncode if done.returncode < 0 else done.returncode
        except LoaderOverride:
            print('REFUSED: otool -L reports the recorded libtkrzw install name, which DYLD_LIBRARY_PATH or DYLD_FALLBACK_LIBRARY_PATH can bypass; unset both to probe the host library.', file=sys.stderr)
            return 1
        except LoaderFailure as error:
            return error.status  # env.sh suppressed the loader command's stderr
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
