"""The host loader resolver shared by native, measurement and compat callers.

Backend labels are advisory: certify the complete set the actual loader resolves,
against the explicitly configured healthy library. No caller infers loader trust.
"""
import json
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
import sys


if not __package__:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))


class RuntimeRefused(RuntimeError):
    """The object's loader cannot be certified against the configured runtime."""


class LoaderOverride(RuntimeError):
    """A dyld override can affect the launcher and probe differently."""


class LoaderFailure(RuntimeError):
    """The host loader could not establish the dependency set."""

    def __init__(self, message, status=1):
        super().__init__(message)
        self.status = status


_DYLD_SELECTION_OVERRIDES = (
    'DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH', 'DYLD_VERSIONED_LIBRARY_PATH',
    'DYLD_FRAMEWORK_PATH', 'DYLD_FALLBACK_FRAMEWORK_PATH', 'DYLD_VERSIONED_FRAMEWORK_PATH',
    'DYLD_IMAGE_SUFFIX', 'DYLD_INSERT_LIBRARIES', 'DYLD_ROOT_PATH',
    'DYLD_SHARED_CACHE_DIR', 'DYLD_SHARED_REGION', 'DYLD_FORCE_PLATFORM',
)
_MACH_HEADER = struct.Struct('<8I')
_MH_MAGIC_64 = 0xfeedfacf
_MH_EXECUTE = 2
_MH_DYLIB = 6
_MH_DYLDLINK = 4
_LC_ID_DYLIB = 0xd
_LC_RPATH = 0x8000001c
_DYLIB_IMPORTS = (0xc, 0x80000018, 0x8000001f, 0x80000023)  # load, weak, re-export, upward


def _darwin_load_command(obj):
    """Load a dylib in a fresh interpreter for certification."""
    return [sys.executable, '-I', '-S', '-c',
            "import runpy, sys; runpy.run_path(sys.argv[1])['_darwin_load_child'](sys.argv[2])",
            str(Path(__file__).resolve()), str(obj)]


def _mach_commands(commands, count, path):
    offset = 0
    for _ in range(count):
        if offset + 8 > len(commands):
            raise LoaderFailure(f'truncated Mach-O load commands in {path}')
        kind, length = struct.unpack_from('<II', commands, offset)
        end = offset + length
        if length < 8 or end > len(commands):
            raise LoaderFailure(f'invalid Mach-O load command in {path}')
        if kind in (0x20, 0x3a):  # LC_LAZY_LOAD_DYLIB[_INFO]
            raise LoaderFailure(f'cannot certify a lazy Mach-O dependency in {path}')
        yield kind, commands[offset:end]
        offset = end
    if offset != len(commands):
        raise LoaderFailure(f'incomplete Mach-O load commands in {path}')


def _mach_command_path(command, minimum, path):
    if len(command) > minimum:
        start = struct.unpack_from('<I', command, 8)[0]
        name, nul, _ = command[start:].partition(b'\0')
        if minimum <= start < len(command) and nul and name:
            return os.fsdecode(name)
    raise LoaderFailure(f'invalid Mach-O path in {path}')


def _mach_dylib_commands(address, path):
    """Read only the install ID and imports from a 64-bit header mapped by dyld."""
    import ctypes
    magic, _, _, _, count, size, _, _ = _MACH_HEADER.unpack(ctypes.string_at(address, _MACH_HEADER.size))
    if magic != _MH_MAGIC_64:
        raise RuntimeError('unsupported Mach-O image header')
    commands = ctypes.string_at(address + _MACH_HEADER.size, size)
    for kind, command in _mach_commands(commands, count, path):
        if kind == _LC_ID_DYLIB or kind in _DYLIB_IMPORTS:
            yield kind, _mach_command_path(command, 24, path)


def _darwin_loaded_tkrzw(obj):
    import ctypes

    dyld = ctypes.CDLL(None)
    dyld._dyld_image_count.restype = ctypes.c_uint32
    dyld._dyld_get_image_name.argtypes = [ctypes.c_uint32]
    dyld._dyld_get_image_name.restype = ctypes.c_char_p
    dyld._dyld_get_image_header.argtypes = [ctypes.c_uint32]
    dyld._dyld_get_image_header.restype = ctypes.c_void_p
    dyld._dyld_get_shared_cache_range.argtypes = [ctypes.POINTER(ctypes.c_size_t)]
    dyld._dyld_get_shared_cache_range.restype = ctypes.c_void_p
    cache_size = ctypes.c_size_t()
    cache = dyld._dyld_get_shared_cache_range(ctypes.byref(cache_size)) or 0
    images = {}
    matches = set()
    count = dyld._dyld_image_count()
    for index in range(count):
        name = dyld._dyld_get_image_name(index)
        address = dyld._dyld_get_image_header(index)
        if not name or not os.path.isabs(name) or not address:
            raise RuntimeError('incomplete dyld image list')
        path = os.fsdecode(name)
        physical = os.path.realpath(path)
        if physical in images:
            raise RuntimeError(f'multiple dyld images for {physical}')
        images[physical] = (path, address)
        if _is_tkrzw_dylib(path) or _is_tkrzw_dylib(physical):
            matches.add(physical)

    # Verify each edge against dyld's image list. With no launcher context we
    # can establish only exact file identities and loader-relative identities.
    # In particular, even a successful @rpath load can use the child's rpaths.
    pending = [(obj, False)]
    visited = set()
    while pending:
        required, tkrzw_import = pending.pop()
        physical = os.path.realpath(required)
        if physical not in images:
            raise RuntimeError(f'dyld did not report an image for {required}')
        path, address = images[physical]
        if tkrzw_import:
            matches.add(physical)
        if physical in visited:
            continue
        visited.add(physical)
        # The system cache is already linked by macOS. Its images still
        # participate in the observed image list above.
        if cache <= address < cache + cache_size.value:
            continue
        for kind, name in _mach_dylib_commands(address, path):
            if kind == _LC_ID_DYLIB:
                if _is_tkrzw_dylib(name):
                    matches.add(physical)
                continue
            if name.startswith(('@executable_path/', '@rpath/')):
                raise RuntimeError(f'{path} can depend on the main executable through {name}')
            if name.startswith('@loader_path/'):
                target = os.path.join(os.path.dirname(path), name.removeprefix('@loader_path/'))
            elif os.path.isabs(name):
                target = name
            else:
                raise RuntimeError(f'cannot certify the install name {name} in {path}')
            pending.append((target, _is_tkrzw_dylib(name)))
    if count != dyld._dyld_image_count():
        raise RuntimeError('dyld image list changed during inspection')
    return sorted(matches)


def _is_tkrzw_dylib(path):
    return re.fullmatch(r'libtkrzw.*\.dylib', os.path.basename(path)) is not None


def _read_mach_image(path, filetype, cpu):
    """Read the native image's static imports, run paths, and install ID."""
    with path.open('rb') as stream:
        header = stream.read(_MACH_HEADER.size)
        if len(header) != _MACH_HEADER.size:
            raise LoaderFailure(f'truncated Mach-O header in {path}')
        magic, architecture, subtype, kind, count, size, flags, _ = _MACH_HEADER.unpack(header)
        # Specialized slices and foreign architectures may use a different
        # dyld shared cache.
        if (magic, architecture, subtype, kind) != (_MH_MAGIC_64, *cpu, filetype) or not flags & _MH_DYLDLINK:
            raise LoaderFailure(f'cannot establish a native Mach-O image for {path}')
        if size > os.fstat(stream.fileno()).st_size - _MACH_HEADER.size:
            raise LoaderFailure(f'truncated Mach-O load commands in {path}')
        commands = stream.read(size)
        if len(commands) != size:
            raise LoaderFailure(f'truncated Mach-O load commands in {path}')
    imports, rpaths, platforms = [], [], set()
    identifier = None
    dylinker = None
    for kind, command in _mach_commands(commands, count, path):
        if kind in _DYLIB_IMPORTS:
            imports.append(_mach_command_path(command, 24, path))
        elif kind == _LC_ID_DYLIB:
            identifier = _mach_command_path(command, 24, path)
        elif kind == _LC_RPATH:
            rpaths.append(_mach_command_path(command, 12, path))
        elif kind == 0xe:  # LC_LOAD_DYLINKER
            dylinker = _mach_command_path(command, 12, path)
        elif kind == 0x27:  # LC_DYLD_ENVIRONMENT
            raise LoaderFailure(f'cannot certify dyld settings embedded in {path}')
        elif kind == 0x32 and len(command) >= 24:  # LC_BUILD_VERSION
            platforms.add(struct.unpack_from('<I', command, 8)[0])
        elif kind == 0x24:  # LC_VERSION_MIN_MACOSX
            platforms.add(1)
    if platforms != {1} or (filetype == _MH_EXECUTE and dylinker != '/usr/lib/dyld'):
        raise LoaderFailure(f'cannot establish the macOS loader for {path}')
    if filetype == _MH_DYLIB and identifier is None:
        raise LoaderFailure(f'missing Mach-O install ID in {path}')
    return imports, rpaths, identifier


def _darwin_anchor(name, loader, executable):
    if os.path.isabs(name):
        return Path(name)
    prefix, _, suffix = name.partition('/')
    if prefix == '@loader_path':
        return loader.parent / suffix.lstrip('/')
    if prefix == '@executable_path':
        return executable.parent / suffix.lstrip('/')
    raise LoaderFailure(f'cannot establish the path {name} from {loader}')


def _darwin_import(name, loader, executable, rpaths, in_cache):
    if name.startswith('@rpath/'):
        candidates = tuple(base / name[7:].lstrip('/') for base in rpaths)
    else:
        candidates = (_darwin_anchor(name, loader, executable),)
    found = set()
    for candidate in candidates:
        path = candidate.resolve()
        if in_cache(os.fsencode(path)):
            found.add((path, True))
        try:
            mode = candidate.stat().st_mode
        except FileNotFoundError:
            continue
        if not stat.S_ISREG(mode):
            raise LoaderFailure(f'non-file candidate {candidate} for {name} in {loader}')
        found.add((path, False))
    if len(found) != 1:
        reason = 'no candidate' if not found else 'multiple candidates'
        raise LoaderFailure(f'{reason} for {name} imported by {loader}')
    return found.pop(), candidates


def _darwin_executable_tkrzw(obj):
    import ctypes

    cpu = {'arm64': (0x100000c, 0), 'x86_64': (0x1000007, 3)}.get(os.uname().machine)
    if cpu is None:
        raise LoaderFailure(f'cannot establish the native architecture for {obj}')
    try:
        in_cache = ctypes.CDLL(None)._dyld_shared_cache_contains_path
    except AttributeError as error:
        raise LoaderFailure('dyld cannot identify the host shared-cache images') from error
    in_cache.argtypes = [ctypes.c_char_p]
    in_cache.restype = ctypes.c_bool

    executable = Path(obj).resolve(strict=True)
    pending = [(executable, (), ())]
    images, imports_by_name = {}, {}
    visited, matches = set(), set()

    def record(name, path):
        # dyld may reuse an image by its install name. Conflicting claims
        # make the result order-dependent, even with one candidate per path.
        if name and (os.path.isabs(name) or name.startswith('@rpath/')):
            if imports_by_name.setdefault(name, path) != path:
                raise LoaderFailure(f'conflicting Mach-O install name {name}')

    while pending:
        path, inherited, ancestors = pending.pop()
        if path not in images:
            images[path] = _read_mach_image(path, _MH_EXECUTE if path == executable else _MH_DYLIB, cpu)
        imports, own_rpaths, identifier = images[path]
        record(identifier, path)
        if identifier and _is_tkrzw_dylib(identifier):
            matches.add(str(path))
        own = tuple(_darwin_anchor(name, path, executable) for name in own_rpaths)
        runpaths = tuple(dict.fromkeys((*own, *inherited)))
        key = path, runpaths
        if key in visited:
            continue
        visited.add(key)
        chain = (*ancestors, path)
        for name in imports:
            (target, cached), candidates = _darwin_import(name, path, executable, runpaths, in_cache)
            record(name, target)
            # A previous load's install ID can satisfy an expanded path
            # even when that path has no corresponding on-disk file.
            for candidate in candidates:
                record(str(candidate), target)
                record(os.path.normpath(candidate), target)
            record(str(target), target)
            if _is_tkrzw_dylib(name) or _is_tkrzw_dylib(target):
                matches.add(str(target))
            # dyld's cache membership establishes a system image even when
            # there is no corresponding file to open. Cycles reuse an image.
            if not cached and target not in chain:
                pending.append((target, runpaths, chain))
    return sorted(matches)


def _darwin_load_child(obj):
    import ctypes

    try:
        _library = ctypes.CDLL(obj)
        print(json.dumps(_darwin_loaded_tkrzw(obj)))
    except (OSError, AttributeError, RuntimeError) as error:
        print(error)
        raise SystemExit(1) from error


def _resolve_darwin(obj, environment, directory):
    overrides = [name for name in _DYLD_SELECTION_OVERRIDES if environment.get(name)]
    if overrides:
        raise LoaderOverride(', '.join(overrides))
    obj = str(directory / obj)
    header = subprocess.run(['otool', '-arch', 'all', '-hv', obj], env=environment, cwd=directory,
                            capture_output=True, text=True, errors='surrogateescape', timeout=30)
    types = re.findall(r'^\s*MH_MAGIC(?:_64)?\s+\S+\s+\S+\s+\S+\s+(\S+)', header.stdout, re.M)
    if not header.returncode and types == ['EXECUTE']:
        return 'Darwin', header, _darwin_executable_tkrzw(obj)
    if 'EXECUTE' in types:
        raise LoaderFailure(f'cannot establish the Mach-O executable slice for {obj}')
    if header.returncode or not types or set(types) != {'DYLIB'}:
        raise LoaderFailure(f'cannot identify {obj} as a Mach-O dylib: {header.stderr.strip()}')
    if len(types) != 1:
        raise LoaderFailure(f'cannot certify universal dylib {obj} without the launcher architecture')

    done = subprocess.run(_darwin_load_command(obj), env=environment, cwd=directory,
                          stdin=subprocess.DEVNULL, capture_output=True, text=True,
                          errors='surrogateescape', timeout=30)
    if done.returncode:
        detail = done.stdout.strip() or done.stderr.strip()
        raise LoaderFailure(detail or f'dyld probe exited with status {done.returncode}')
    try:
        matches = json.loads(done.stdout)
    except ValueError as error:
        raise LoaderFailure('dyld probe did not report its dependency set') from error
    if not isinstance(matches, list) or any(not isinstance(path, str) or not os.path.isabs(path)
                                            for path in matches):
        raise LoaderFailure('dyld probe returned an invalid dependency set')
    return 'Darwin', done, matches


def resolve(obj, *, env=None, cwd=None, probe=False):
    """Read loader dependencies once for certification or pre-certification host probes.

    PROBE preserves the earlier loader helper's historical libtkrzw.so.1 view;
    certification always checks the full resolved dependency set.
    """
    environment = dict(os.environ if env is None else env)
    directory = Path.cwd() if cwd is None else Path(cwd).resolve()
    obj = str(obj)
    try:
        host = subprocess.run(['uname', '-s'], env=environment, cwd=directory,
                              capture_output=True, text=True, timeout=30).stdout.strip()
        if host == 'Darwin':
            return _resolve_darwin(obj, environment, directory)
        done = subprocess.run(['ldd', obj],
                              env=environment, cwd=directory, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, text=True, timeout=30)
    except OSError as error:
        from tools.dev.native import launch_status
        raise LoaderFailure(str(error), launch_status(error)) from error
    except subprocess.TimeoutExpired as error:
        raise LoaderFailure(str(error)) from error
    matches = []
    for line in done.stdout.splitlines():
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
    except LoaderOverride as error:
        refuse(f'<dyld override set: {error}>', '<loader context without overrides>')
    except LoaderFailure as error:
        raise RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to <unknown>: {error}') from error
    if not matches and done.returncode == 0:
        if host != 'Darwin' and 'libtkrzw' in done.stdout:
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
    """Preserve the python -m tools.tkrzw.runtime CLI, including child execution and signal statuses."""
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
        except LoaderOverride as error:
            print(f'REFUSED: unset dyld overrides before probing: {error}', file=sys.stderr)
            return 1
        except LoaderFailure as error:
            return error.status  # the earlier loader helper suppressed the loader command's stderr
    quiet = print_path = False
    while args and args[0] in ('--quiet-stderr', '--print-path'):
        quiet |= args[0] == '--quiet-stderr'
        print_path |= args.pop(0) == '--print-path'
    if len(args) < 4:
        print('usage: python -m tools.tkrzw.runtime [--quiet-stderr] [--print-path] CELL RUNNER SIDE OBJECT [-- COMMAND...]', file=sys.stderr)
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
