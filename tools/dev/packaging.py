"""Install, validate and assemble the drop-in packages (#1292).

One layout and process adapter serve every entry point.
Cargo and package builders run supervised (slot.run_command); captured tools use
pool.UnitContext, which reaps their process groups on cancellation.
"""

if __name__ == '__main__':
    from .entry import require_utf8_mode

    require_utf8_mode()

from contextlib import chdir, contextmanager
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib

from . import cell, pool, slot
from .runners import Finished, emit, guarded, status, write

ROOT = Path(__file__).resolve().parents[2]
# Each installed library, with the name that its C crate declares in [package.metadata.capi.library].
LIBRARIES = {'libpinyin': 'pinyin', 'libzhuyin': 'zhuyin'}
HEADERS = ('novel_types.h', 'pinyin_custom2.h')
LOAD_CHILD = r'''
import ctypes, os, sys
path, symbol = sys.argv[1:]
class Info(ctypes.Structure):
    _fields_ = [('fname', ctypes.c_char_p), ('fbase', ctypes.c_void_p),
                ('sname', ctypes.c_char_p), ('saddr', ctypes.c_void_p)]
try:
    function = getattr(ctypes.CDLL(path), symbol)
except (OSError, AttributeError) as error:
    print(error)
    sys.exit(1)
info = Info()
libc = ctypes.CDLL(None)
libc.dladdr.argtypes = [ctypes.c_void_p, ctypes.POINTER(Info)]
if not libc.dladdr(ctypes.cast(function, ctypes.c_void_p), ctypes.byref(info)) or not info.fname:
    print(f'dladdr could not place {symbol}')
    sys.exit(1)
loaded = os.path.realpath(info.fname.decode(errors='surrogateescape'))
if loaded != os.path.realpath(path):
    print(f'{symbol} resolved in {loaded}, not in {path}')
    sys.exit(1)
'''
ENTRY_POINTS = {'libpinyin': 'pinyin_init', 'libzhuyin': 'zhuyin_init'}
# The libtool current.revision of [package.metadata.capi.library].version in both C-ABI crates:
# the .pc files' libpinyin_binary_version and libzhuyin_binary_version.
BINARY_VERSION = '15.0'
NAMES = ('install', 'pc-version', 'check-pc-version', 'check-pc-metadata',
         'distro-tkrzw-library', 'check-library-mode', 'check-installed', 'release-version', 'release-recipe', 'sizes',
         'trainer-layout')

# libpinyin 074a2219 utils/{storage,segment,training}/Makefile.am.
TRAINER_TOOLS = {
    'storage': ('gen_binary_files', 'import_interpolation', 'export_interpolation',
                'gen_pinyin_table', 'gen_zhuyin_table'),
    'segment': ('ngseg', 'spseg', 'mergeseq'),
    'training': ('gen_unigram', 'gen_ngram', 'gen_deleted_ngram', 'gen_k_mixture_model',
                 'estimate_interpolation', 'estimate_k_mixture_model', 'merge_k_mixture_model',
                 'prune_k_mixture_model', 'import_k_mixture_model', 'export_k_mixture_model',
                 'k_mixture_model_to_interpolation', 'validate_k_mixture_model',
                 'eval_correction_rate'),
}


def trainer_layout(argv, tools):
    """Copy built tools and the configuration template into a new trainer tree."""
    import argparse
    from .packaging_templates import TRAINER_MAKEFILE

    parser = argparse.ArgumentParser(prog='just trainer-layout')
    parser.add_argument('target', type=Path, help='new output directory, not under a directory with a .git entry')
    build = Path(tools.env.get('CARGO_TARGET_DIR') or tools.root / 'target')
    parser.add_argument('--bin-dir', type=Path, default=build / 'debug',
                        help='directory containing built oxpinyin-utils binaries (default: target/debug)')
    args = parser.parse_args(argv)
    target = args.target.absolute()
    if entry_exists(target):
        fail(f'error: trainer layout target already exists: {target}', 2)
    target = target.resolve()
    # Check every ancestor, including beyond nested repositories. A .git
    # entry can be a directory, a gitfile (linked worktree or submodule) or
    # a symlink. A work tree with no .git entry (core.worktree) is not detected.
    for parent in target.parents:
        if entry_exists(parent / '.git'):
            fail(f'error: trainer layout target is under a directory with a .git entry: {parent}', 2)
    sources = [(Path('utils') / group / name, args.bin_dir / name)
               for group, names in TRAINER_TOOLS.items() for name in names]
    for _, source in sources:
        if not source.is_file() or not os.access(source, os.X_OK):
            fail(f'error: missing executable {source}; build oxpinyin-utils --bins first', 2)
    template = (tools.root / 'data/table.conf.in').read_bytes()
    # Claim the directory before writing. In particular, never merge into a
    # trainer tree containing a user's models or follow an existing target link.
    target.mkdir(parents=True)
    for relative, source in sources:
        destination = target / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    data = target / 'data'
    data.mkdir()
    (data / 'Makefile').write_text(TRAINER_MAKEFILE, encoding='utf-8')
    (data / 'table.conf.in').write_bytes(template)
    emit(f'trainer layout: copied {len(sources)} utilities to {target}; populate data/ before training')
    return 0


def entry_exists(path):
    """Whether path names a directory entry; a final symlink is not followed.

    Refuse when the entry cannot be inspected: an ancestor that denies search
    permission or is not a directory proves neither answer.
    """
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    except OSError as error:
        fail(f'error: trainer layout cannot inspect {path}: {error}', 2)
    return True


def fail(message='', code=1):
    if message:
        emit(message, error=True)
    raise Finished(code)


class Tools:
    """Run packaging commands with the selected directory and environment."""
    def __init__(self, env=None, *, root=ROOT):
        self.root = Path(root)
        self.env = dict(os.environ if env is None else env)

    def capture(self, argv, *, cwd=None, env=None, merge_stderr=False, input=b''):
        context = pool.UnitContext('packaging', slot.CANCELLED,
            environment=dict(self.env if env is None else env))
        try:
            done = context.launch(list(map(str, argv)), cwd=cwd, merge_stderr=merge_stderr, input=input)
        except PermissionError as error:
            fail(str(error), 126)
        return done._replace(status=status(done.status))

    def run(self, argv, **options):
        done = self.capture(argv, **options)
        write(done.stdout)
        write(done.stderr, error=True)
        if done.status:
            raise Finished(done.status)
        return done.stdout.decode(errors='surrogateescape').rstrip('\n')

    def heavy(self, argv, *, cwd=None, env=None):
        with chdir(cwd or Path.cwd()):
            try:
                code = status(slot.run_command(list(map(str, argv)), env=self.env if env is None else env))
            except PermissionError as error:
                fail(str(error), 126)
        if code:
            raise Finished(code)

    def load(self, path, symbol):
        """dlopen the library in a child with the dyld search overrides removed and look one symbol up.

        Returns the exit status and any diagnostic on failure, else ''. The child asks dladdr where
        the symbol came from, so a different image of the same install name cannot satisfy the check.
        """
        env = {name: value for name, value in self.env.items() if not name.startswith('DYLD_')}
        done = self.capture([sys.executable, '-I', '-c', LOAD_CHILD, str(path), symbol], env=env)
        if done.status == 0:
            return ''
        failure = f'load check exited with status {done.status}'
        diagnostic = (done.stdout.decode(errors='surrogateescape').strip()
                      or done.stderr.decode(errors='surrogateescape').strip())
        return f'{failure}: {diagnostic}' if diagnostic else failure

    def which(self, name):
        return shutil.which(name, path=self.env.get('PATH'))

    def text(self, argv, **options):
        done = self.capture(argv, **options)
        write(done.stderr, error=True)
        if done.status:
            raise Finished(done.status)
        return done.stdout.decode(errors='surrogateescape').rstrip('\n')


def option(argv, index):
    """The shell's ${next:-}, including a missing final option value."""
    value, separator, text = argv[index].partition('=')
    if separator:
        return value, text, index + 1
    return value, argv[index + 1] if index + 1 < len(argv) else '', index + 2


def version(root=ROOT):
    record = root / 'tools/oracle/oracle-pin.txt'
    try:
        text = record.read_text(newline='')
    except OSError as error:
        fail(f"sed: can't read {record}: {error.strerror}", 2)
    values = re.findall(r'^libpinyin_tag=(.*)$', text, re.M)
    value = '\n'.join(values)
    if not re.search(r'^[0-9]+\.[0-9]+\.[0-9]+$', value, re.M):
        fail(f'error: {record}: no well-formed libpinyin_tag=<major>.<minor>.<micro> line')
    return value


def cargo_metadata(root, env=None):
    """The Cargo metadata of the workspace at root, without its dependencies.

    Nothing keeps the result between calls: one path can hold one revision and then another
    in one process, as the size build's source directory does. An operation that needs several
    answers about one tree reads once and passes the result down as METADATA.
    """
    command = ['cargo', 'metadata', '--locked', '--no-deps', '--format-version', '1']
    try:
        done = subprocess.run(command, cwd=root, env=env, capture_output=True)
    except FileNotFoundError as error:
        fail(f'error: {root}: cannot run cargo metadata: {error.strerror}', 127)
    except PermissionError as error:
        fail(str(error), 126)
    if done.returncode:
        write(done.stderr, error=True)
        fail(f'error: {root}: cargo metadata failed', status(done.returncode))
    return json.loads(done.stdout)


def capi_package(root, library, env=None, *, metadata=None):
    """The package at root whose [package.metadata.capi.library] name is library ('pinyin' or 'zhuyin')."""
    metadata = metadata or cargo_metadata(root, env)
    packages = [package for package in metadata['packages']
                if (((package.get('metadata') or {}).get('capi') or {}).get('library') or {}).get('name') == library]
    if len(packages) != 1:
        fail(f"error: {root}: {len(packages)} packages declare the C library '{library}' in "
             '[package.metadata.capi.library]; exactly one must')
    return packages[0]


def crate_dir(root, library, env=None, *, metadata=None):
    """The directory of the C crate that installs library ('pinyin' or 'zhuyin') in the source tree at root.

    The tree's own Cargo metadata names the crate, so no package name and no layout is assumed.
    """
    metadata = metadata or cargo_metadata(root, env)
    manifest = Path(capi_package(root, library, metadata=metadata)['manifest_path'])
    return Path(root) / manifest.parent.relative_to(metadata['workspace_root'])


def companion_headers(*, stage=False, root=ROOT):
    metadata = cargo_metadata(root)
    crates = [crate_dir(root, library, metadata=metadata) for library in LIBRARIES.values()]
    for name in HEADERS:
        paths = [crate / name for crate in crates]
        try:
            same = paths[0].read_bytes() == paths[1].read_bytes()
        except OSError:
            same = False
        if not same:
            message = (f"error: companion header '{name}' is not identical to its target in the other crate; "
                       'the checkout lacks git symlink support')
            if not stage:
                message += f':\n         {paths[0]}\n         {paths[1]}\n'
                message += ('       the zhuyin crate tracks the header as a symlink to the pinyin crate; '
                            'check out again with core.symlinks=true.')
            fail(message)


@dataclass
class Layout:
    library: str
    prefix: str
    libdir: str
    includedir: str
    destdir: str
    pc_libdir: str
    pc_includedir: str
    target: Path
    triple: str
    profile: str
    passthru: list[str]

    @property
    def qualified(self):
        return self.target / self.triple / self.profile

    @property
    def pcdir(self):
        return Path(self.destdir + self.libdir) / 'pkgconfig'

    @classmethod
    def parse(cls, argv, tools):
        usage = ('usage: just packaging install <libpinyin|libzhuyin> --prefix=DIR '
                 '[--libdir=DIR] [--includedir=DIR] [--destdir=DIR] [-- <extra cargo cinstall args>]\n'
                 '       one library per invocation; run twice to install both')
        if not argv or argv[0] not in LIBRARIES:
            fail(usage, 2)
        library = argv[0]
        values = dict(prefix='', libdir='', includedir='', destdir='')
        extra = []
        index = 1
        while index < len(argv):
            word = argv[index]
            if word == '--':
                extra += argv[index + 1:]
                break
            if word.split('=')[0].removeprefix('--') in values and word.startswith('--'):
                key, value, index = option(argv, index)
                values[key[2:]] = value
            else:
                extra.append(word)
                index += 1
        if not values['prefix']:
            fail(usage, 2)
        prefix = values['prefix']
        target = tools.env.get('CARGO_TARGET_DIR') or str(tools.root / 'target')
        triple, profile, passthru = '', 'release', []
        index = 0
        while index < len(extra):
            word = extra[index]
            key = word.split('=')[0]
            if key in ('--target-dir', '--target', '--profile'):
                key, value, index = option(extra, index)
                if key == '--target-dir':
                    target = value or (target if '=' not in word else '')
                    continue
                if key == '--target':
                    triple = value
                else:
                    value = value or ('release' if '=' not in word else '')
                    profile = 'debug' if value == 'dev' else value
                passthru += [word] if '=' in word else [key, value]
            else:
                if word in ('--debug', '--release'):
                    profile = 'debug' if word == '--debug' else 'release'
                passthru.append(word)
                index += 1
        with chdir(tools.root):
            target_path = Path(target) if target else tools.root / 'target'
            if not target_path.is_absolute():
                target_path = tools.root / target_path
            if target:
                tools.run(['mkdir', '-p', '--', target_path])
            # The shell's cd + pwd retains logical aliases such as /var.
            target_path = Path(os.path.abspath(target_path))
            # cargo cinstall runs in the crate directory, so a relative
            # destdir is made absolute here, from the repository root.
            destdir = values['destdir']
            if destdir and not os.path.isabs(destdir):
                destdir = os.path.abspath(destdir)
        triple = triple or tools.env.get('CARGO_BUILD_TARGET') or ''
        if not triple:
            triple = '\n'.join(re.findall(r'^host: (.*)$', tools.text(['rustc', '-vV']), re.M))
        if triple:
            triple = triple.rstrip('/') or '/'
            if triple != '/':
                triple = triple.rsplit('/', 1)[-1]
            if triple.endswith('.json') and triple != '.json':
                triple = triple[:-5]
        if not triple:
            fail("error: could not determine the build target (rustc -vV had no 'host:' line)")
        return cls(library, prefix, values['libdir'] or prefix + '/lib',
                   values['includedir'] or prefix + '/include', destdir,
                   values['libdir'] or '${exec_prefix}/lib', values['includedir'] or '${prefix}/include',
                   target_path, triple, profile, passthru)

    def features(self):
        result, index = '', 0
        while index < len(self.passthru):
            if self.passthru[index].split('=')[0] == '--features':
                _, value, index = option(self.passthru, index)
                result = (result + ',' if result else '') + value
            else:
                index += 1
        return result


def pc_text(library, backend, root=ROOT):
    """The .pc file of LIBRARY for BACKEND, with its install-time placeholders left in place.

    The crate's .pc.in is the pin's template under a `#` header. This fills what the pin's
    configure knows before an install: @VERSION@ from the pin record, the binary version and
    the backend's database format. The header documents the source file, so no `#` line
    reaches the installed file. No build script and no Cargo build directory takes part.
    """
    path = crate_dir(root, LIBRARIES[library]) / (library + '.pc.in')
    text = path.read_bytes().decode()
    for name, value in [('VERSION', version(root)), ('LIBPINYIN_BINARY_VERSION', BINARY_VERSION),
                        ('DATABASE_FORMAT', cell.BACKEND_FORMATS[backend])]:
        text = text.replace(f'@{name}@', value)
    lines = [line.removesuffix('\r') for line in text.removesuffix('\n').split('\n')]
    body = '\n'.join(line for line in lines if not line.lstrip().startswith('#'))
    return (body.lstrip('\n') + '\n').encode()


def install(argv, tools):
    with chdir(tools.root):
        return install_library(argv, tools)


def install_library(argv, tools):
    layout = Layout.parse(argv, tools)
    # cargo-c reads the manifest's version declarations during the install below.
    # The pin record is read first: it needs no Cargo.
    problems = version_problems(tools.root)
    if problems:
        fail('\n'.join(problems))
    companion_headers(root=tools.root)
    selected = next((peer for peer in cell.BACKENDS if peer in layout.features().replace(',', ' ').split()),
                    None) or cell.declared_default(tools.root)
    crate = crate_dir(tools.root, LIBRARIES[layout.library])
    system = tools.text(['uname', '-s'])
    if system == 'Linux':
        return install_linux(layout, crate, selected, tools)
    tools.run(['mkdir', '-p', layout.pcdir])
    command = ['cargo', 'cinstall', f'--prefix={layout.prefix}', f'--libdir={layout.libdir}',
               f'--includedir={layout.includedir}', f'--target-dir={layout.target}']
    if layout.destdir:
        command.append(f'--destdir={layout.destdir}')
    env = None
    if system == 'Darwin':
        # rustc, cc and cargo-c read the target from the environment; the relink
        # below keeps the value that cargo-c's dylib records.
        env = {**tools.env, 'MACOSX_DEPLOYMENT_TARGET': deployment_target(tools)}
    tools.heavy([*command, *layout.passthru], cwd=crate, env=env)
    data = pc_text(layout.library, selected, tools.root)
    for name, value in [('prefix', layout.prefix), ('exec_prefix', '${prefix}'),
                        ('libdir', layout.pc_libdir), ('includedir', layout.pc_includedir)]:
        data = data.replace(f'@{name}@'.encode(), os.fsencode(value))
    pc = layout.pcdir / (layout.library + '.pc')
    pc.write_bytes(data)
    emit(f'installed complete pkg-config file: {pc}')
    if system == 'Darwin' and layout.triple.endswith('-apple-darwin'):
        dylib = Path(layout.destdir + layout.libdir) / (layout.library + '.15.0.0.dylib')
        relink_darwin(layout, dylib, tools, selected)
        host = '\n'.join(re.findall(r'^host: (.*)$', tools.text(['rustc', '-vV']), re.M))
        if layout.triple.split('-')[0] != host.split('-')[0]:
            emit(f'load check skipped: target {layout.triple} is not the running architecture ({host}); '
                 'the in-process dlopen cannot load it')
            return 0
        problem = tools.load(dylib, ENTRY_POINTS[layout.library])
        if problem:
            fail(f'error: the installed {dylib} does not load:\n         {problem}')
        emit(f'loaded {dylib} and found {ENTRY_POINTS[layout.library]}')
    return 0


def install_linux(layout, crate, backend, tools):
    """Install one library through tools/dev/packaging.mk, the release recipes' build and install.

    The Makefile links the shared object from the cargo-c staticlib under the crate's .ver
    script with the environment's LDFLAGS, verifies its exports, and writes the complete .pc.
    """
    if not tools.which('make'):
        fail("error: GNU make is required; install the distribution's make package")
    command = ['make', '-f', tools.root / 'tools/dev/packaging.mk', 'install-' + layout.library,
               f'BACKEND={backend}', f'prefix={layout.prefix}', f'CARGO_TARGET_DIR={layout.target}',
               f'TRIPLE={layout.triple}', f'PROFILE={layout.profile}',
               # make expands the value once more, so a literal $ is doubled.
               f'CARGO_FLAGS={shlex.join(layout.passthru).replace("$", "$$")}', 'FEATURES=',
               f'{layout.library}_crate={crate.relative_to(tools.root).as_posix()}']
    # A directory that was not given keeps the pin's prefix-relative .pc value.
    if layout.pc_libdir == layout.libdir:
        command.append(f'libdir={layout.libdir}')
    if layout.pc_includedir == layout.includedir:
        command.append(f'includedir={layout.includedir}')
    if layout.destdir:
        command.append(f'DESTDIR={layout.destdir}')
    if tools.env.get('LDFLAGS'):
        # make and then the recipe's shell would each expand the environment's value. Each
        # flag is quoted for the shell and a literal $, as in $ORIGIN, is doubled for make.
        try:
            flags = shlex.split(tools.env['LDFLAGS'])
        except ValueError as error:
            fail(f"error: cannot split LDFLAGS {tools.env['LDFLAGS']!r}: {error}", 2)
        command.append('LDFLAGS=' + shlex.join(flags).replace('$', '$$'))
    tools.heavy(command, cwd=tools.root)
    return 0


@dataclass(frozen=True)
class DarwinIdentity:
    """The cargo-c dylib identity that a replacement must preserve."""
    install_name: str
    compatibility_version: str
    current_version: str
    architecture: str
    deployment_target: str

    @classmethod
    def read(cls, library, tools):
        commands = tools.text(['otool', '-l', library]).split('Load command')

        def field(command, name):
            match = re.search(r'^\s*' + re.escape(name) + r'\s+(.+?)(?: \(offset \d+\))?$', command, re.M)
            if not match:
                fail(f'error: {library}: missing Mach-O {name}')
            return match[1]

        identity = next((block for block in commands if re.search(r'cmd LC_ID_DYLIB\s', block)), '')
        build = next((block for block in commands if re.search(r'cmd LC_BUILD_VERSION\s', block)), '')
        if build:
            if field(build, 'platform') not in ('1', 'MACOS'):
                fail(f'error: {library}: expected a macOS dylib')
            minimum = field(build, 'minos')
        else:
            build = next((block for block in commands if re.search(r'cmd LC_VERSION_MIN_MACOSX\s', block)), '')
            minimum = field(build, 'version')
        architecture = tools.text(['lipo', '-archs', library]).split()
        if len(architecture) != 1 or architecture[0] not in ('arm64', 'x86_64'):
            fail(f'error: {library}: expected one arm64 or x86_64 Mach-O slice')
        return cls(field(identity, 'name'), field(identity, 'compatibility version'),
                   field(identity, 'current version'), architecture[0], minimum)


def deployment_target(tools):
    """The macOS version of the build machine, unless MACOSX_DEPLOYMENT_TARGET names one."""
    return tools.env.get('MACOSX_DEPLOYMENT_TARGET') or tools.text(['sw_vers', '-productVersion']).strip()


def relink_darwin(layout, destination, tools, backend):
    """Link once under our allowlist, outside rustc's additive export lists."""
    from .abi import darwin_export_trie, darwin_exported_symbols

    archive = layout.qualified / (layout.library + '.a')
    ver = crate_dir(tools.root, LIBRARIES[layout.library]) / (layout.library + '.ver')
    _, names = version_exports(ver)
    expected = set(names)
    if not expected or any(not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', name) for name in expected):
        fail(f'error: {ver}: expected explicit global symbol names')
    if not archive.is_file():
        fail(f'error: staticlib not found: {archive}')
    identity = DarwinIdentity.read(destination, tools)
    glib = optional_pkg_config(['--libs', 'glib-2.0', 'gobject-2.0'], tools)
    flags = glib.stdout.decode().split() if not glib.status else ['-lglib-2.0', '-lgobject-2.0']
    flags += backend_flags(backend, tools, '-lc++', 'darwin')
    # The link always takes libc++. tkrzw's pkg-config flags and the Kyoto
    # Cabinet flags can already name it; ld warns about a repeated library.
    if '-lc++' not in flags:
        flags.append('-lc++')
    with tempfile.TemporaryDirectory(prefix=destination.name + '.relink.', dir=destination.parent) as scratch:
        exports, temporary = Path(scratch) / 'exports', Path(scratch) / destination.name
        exports.write_text(''.join('_' + name + '\n' for name in sorted(expected)))
        # Keep the final link's LTO DWARF until dsymutil has consumed it.
        debug = tools.env.get('OXPINYIN_DSYM') == '1'
        lto = ['-Wl,-object_path_lto,' + str(Path(scratch) / 'lto.o')] if debug else []
        tools.heavy([tools.env.get('CC') or 'cc', '-dynamiclib', '-o', temporary,
                     '-arch', identity.architecture, '-mmacosx-version-min=' + identity.deployment_target,
                     '-install_name', identity.install_name,
                     '-Wl,-compatibility_version,' + identity.compatibility_version,
                     '-Wl,-current_version,' + identity.current_version,
                     '-Xlinker', '-exported_symbols_list', '-Xlinker', exports,
                     '-Xlinker', '-force_load', '-Xlinker', archive, '-Wl,-dead_strip',
                     *flags, *lto, '-liconv', '-lresolv'])
        actual = darwin_exported_symbols(tools.text(['nm', '-gU', temporary]))
        try:
            trie = darwin_export_trie(tools.text(['dyld_info', '-exports', temporary]))
        except ValueError as error:
            fail(str(error))
        if actual != expected or trie != expected:
            fail(f'error: relinked export set differs from {ver}:\n'
                 f'  missing: {sorted(expected - actual | expected - trie)}\n'
                 f'  extra: {sorted(actual - expected | trie - expected)}')
        if DarwinIdentity.read(temporary, tools) != identity:
            fail(f'error: relinked Mach-O identity differs from {destination}')
        if debug:
            tools.run(['xcrun', 'dsymutil', temporary, '-o', str(destination) + '.dSYM'])
        os.replace(temporary, destination)
    emit(f'relinked {destination}: exactly {len(expected)} exports (backend: {backend})')


def optional_pkg_config(arguments, tools):
    try:
        return tools.capture(['pkg-config', *arguments])
    except OSError:
        return pool.Completed(127, b'', b'')


def macos_tkrzw_modules(root):
    """Read the macOS tkrzw pkg-config dependencies from the product revision."""
    manifest = tomllib.loads((Path(root) / 'Cargo.toml').read_text(encoding='utf-8'))
    macos = manifest['package']['metadata']['system-deps']['cfg(target_os = "macos")']
    return [module for module, spec in macos.items() if spec.get('feature') == 'tkrzw']


def backend_flags(backend, tools, cxx, platform):
    """The backend's link flags; CXX is the C++ runtime the Kyoto Cabinet shim was built against."""
    if backend == 'bdb':
        return ['-ldb']
    modules = [backend]
    if backend == 'tkrzw' and platform == 'darwin':
        # Match the macOS system-deps declaration used by cargo-c's staticlib.
        modules.extend(macos_tkrzw_modules(tools.root))
    done = optional_pkg_config(['--libs', *modules], tools)
    if backend == 'tkrzw' and platform == 'darwin' and done.status:
        fail('error: pkg-config must resolve ' + ', '.join(modules) + ' for the macOS link', 2)
    flags = done.stdout.decode().split() if not done.status else ['-l' + backend]
    # Kyoto Cabinet's pkg-config output can already name the C++ runtime.
    return [*flags, *([cxx] if backend == 'kyotocabinet' and cxx not in flags else [])]


def version_exports(ver):
    text = Path(ver).read_text()
    nodes = re.findall(r'^([A-Za-z_][A-Za-z0-9_]*) \{.*', text, re.M)
    if not nodes:
        raise ValueError(f'no version node found in {ver}')
    selected, lines = False, []
    for line in text.splitlines():
        if 'global:' in line:
            selected = True
        if selected:
            lines.append(line.replace('global:', '').replace('local:', ''))
        if selected and 'local:' in line:
            selected = False
    symbols = [''.join(part.split()) for part in '\n'.join(lines).split(';')]
    return nodes[0], sorted(symbol for symbol in symbols if symbol)


def linux_symbol_names(text: str, node: str):
    """Yield names from nm -D --defined-only, omitting the linker's absolute version node."""
    for row in text.splitlines():
        if row.strip():
            _, kind, name = row.split(maxsplit=2)
            if (kind, name) != ('A', node):
                yield name


def linux_exports(text: str, node: str) -> list[str]:
    return [name.partition('@')[0] for name in linux_symbol_names(text, node)]


def version_problems(root=ROOT):
    """Each drop-in version declaration that differs from the pin record, as an error line."""
    want, problems = version(root), []
    metadata = cargo_metadata(root)
    for library in LIBRARIES.values():
        path = crate_dir(root, library, metadata=metadata) / 'Cargo.toml'
        text = '\n'.join(line for line in path.read_text().splitlines() if not re.match(r'^\s*#', line))
        for value in re.findall(r'"(libpinyin-[^"]*)"', text):
            if value != 'libpinyin-' + want:
                problems.append(f'error: {path} declares "{value}", want "libpinyin-{want}"')
        if f'"libpinyin-{want}"' not in text:
            problems.append(f'error: {path} declares no "libpinyin-{want}" include subdirectory')
        section = ''
        versions = []
        for line in text.splitlines():
            if line.startswith('['):
                section = line
            if section == '[package.metadata.capi.pkg_config]' and re.match(r'^version\s*=', line):
                versions.append(re.sub(r'.*=\s*"|".*', '', line))
        got = '\n'.join(versions)
        if got != want:
            problems.append(f'error: {path} [package.metadata.capi.pkg_config] version is "{got}", want "{want}"')
    for library, name in LIBRARIES.items():
        path = crate_dir(root, name, metadata=metadata) / (library + '.pc.in')
        text = '\n'.join(line for line in path.read_text().splitlines() if not line.startswith('#'))
        if re.search(r'[0-9]+\.[0-9]+\.[0-9]+', text):
            problems.append(f'error: {path} carries a literal version; it must use @VERSION@')
    return problems


def check_version(argv, tools):
    problems = version_problems(tools.root)
    for problem in problems:
        emit(problem, error=True)
    if problems:
        return 1
    emit(f'OK: every drop-in version declaration is {version(tools.root)} (tools/oracle/oracle-pin.txt)')
    return 0


def positional(argv, names):
    for index, name in enumerate(names):
        if index >= len(argv) or not argv[index]:
            fail(f'error: {name} required')
    return argv[:len(names)]


def expand(template, values):
    """Only substitute the uppercase packaging fields; RPM/shell syntax is data."""
    return re.sub(r'\$(?:\{([A-Z_]+)\}|([A-Z_]+))',
                  lambda match: str(values[match[1] or match[2]]), template).replace('\\$', '$')


@contextmanager
def temporary_directory(tools, template=None):
    path = Path(tools.text(['mktemp', '-d', *([template] if template else [])]))
    try:
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)


def resolve_deb_library(soname, tools):
    cache = tools.text(['ldconfig', '-p'])
    for line in cache.splitlines():
        fields = line.split()
        if fields and fields[0] == soname:
            path = os.path.realpath(fields[-1])
            break
    else:
        fail(f'error: soname {soname} not in ldconfig cache')
    done = tools.capture(['dpkg', '-S', '--', path])
    package = done.stdout.decode().split('\n')[0].split(':')[0]
    if done.status or not package:
        fail(f'error: no package owns {path} ({soname})')
    return path, package


def distro_tkrzw_library(argv, tools):
    """Print the distro's dpkg-owned libtkrzw.so.1.

    With --github-env, also append OXPINYIN_TKRZW_LIBRARY=<path> to the $GITHUB_ENV file, as
    the release workflow's runtime selection needs."""
    if argv not in ([], ['--github-env']):
        fail('usage: distro-tkrzw-library [--github-env]', 2)
    path, _ = resolve_deb_library('libtkrzw.so.1', tools)
    if argv:
        target = os.environ.get('GITHUB_ENV')
        if not target:
            fail('error: --github-env needs GITHUB_ENV', 2)
        with open(target, 'a', encoding='utf-8') as stream:
            stream.write(f'OXPINYIN_TKRZW_LIBRARY={path}\n')
    emit(path)
    return 0


# The pin installs its shared libraries with mode 0755 (libtool's install -m 755).
LIBRARY_MODE = 0o755


def check_library_mode(argv, tools):
    """Require MODE (octal; default LIBRARY_MODE) on each installed <library>.so.15.0.0 under LIBDIR.

    A distribution's driver can set its own mode: dh_fixperms installs shared libraries 0644,
    as Debian's libpinyin15 ships them. The mode is the file's own permission bits, as
    `stat -c %a` prints them, not a link target's."""
    if len(argv) not in (1, 2) or not argv[0]:
        fail('usage: check-library-mode LIBDIR [MODE]', 2)
    libdir = argv[0]
    try:
        want = int(argv[1], 8) if len(argv) == 2 else LIBRARY_MODE
    except ValueError:
        fail(f'error: MODE {argv[1]!r} is not an octal mode', 2)
    wrong = []
    for library in LIBRARIES:
        path = Path(libdir) / f'{library}.so.15.0.0'
        try:
            mode = path.lstat().st_mode & 0o7777
        except OSError as error:
            fail(f'error: cannot read the mode of {path}: {error.strerror}')
        if mode != want:
            wrong.append(f'error: {path} has mode {mode:o}, not {want:o}')
        else:
            emit(f'{path}: mode {mode:o}')
    if wrong:
        fail('\n'.join(wrong))
    return 0


def release_version(argv, tools):
    import semver

    if len(argv) != 2 or argv[0] not in ('debian', 'fedora', 'arch'):
        fail('usage: release-version {debian,fedora,arch} TAG', 2)
    distro, tag = argv
    try:
        parsed = semver.Version.parse(tag.removeprefix('v'))
    except ValueError:
        fail(f'error: invalid release tag {tag!r}', 2)
    base = f'{parsed.major}.{parsed.minor}.{parsed.patch}'
    release = base
    if parsed.prerelease:
        suffix = parsed.prerelease if distro == 'debian' else parsed.prerelease.replace('-', '_')
        # vercmp sorts a letter immediately after the patch number before the bare version.
        release += ('pre.' if distro == 'arch' else '~') + suffix
    emit(f'VERSION={release}')
    emit(f'BASE={base}')
    return 0


MAINTAINER = 'Shenghao Chen <shenghaoc@outlook.com>'
HOMEPAGE = 'https://github.com/shenghaoc/oxpinyin'
# The C++ bridge's build dependencies per distribution and backend.
BACKEND_BUILD_DEPS = {
    'debian': {'tkrzw': 'libtkrzw-dev, liblzma-dev, liblz4-dev, libzstd-dev, zlib1g-dev',
               'kyotocabinet': 'libkyotocabinet-dev'},
    'fedora': {'kyotocabinet': 'kyotocabinet-devel'},
    'arch': {'kyotocabinet': 'kyotocabinet'},
}
LICENSE_FILE = re.compile(r'(?:licen[cs]e|copying|copyright|notice|unlicense)(?:[-._].*)?', re.I)


@dataclass
class Crate:
    """A registry crate that the shipped libraries link, as cargo vendor stores it."""
    name: str
    version: str
    license: str
    directory: str
    files: list[str]
    notices: list[str]


def host_triple(tools):
    triple = '\n'.join(re.findall(r'^host: (.*)$', tools.text(['rustc', '-vV']), re.M))
    if not triple:
        fail("error: could not determine the build target (rustc -vV had no 'host:' line)")
    return triple


def shipped_crates(backend, vendor, tools):
    """The registry crates linked into the two libraries for BACKEND, read from the vendored tree.

    Build scripts, dev-dependencies and proc-macros run only at build time and are not linked.
    """
    metadata = cargo_metadata(tools.root)
    packages = [capi_package(tools.root, library, metadata=metadata)['name'] for library in LIBRARIES.values()]
    features = ','.join(f'{package}/{feature}' for package in packages for feature in (backend, 'capi'))
    command = ['cargo', 'tree', '--locked', '--quiet', *[word for package in packages for word in ('-p', package)],
               '--no-default-features', '--features', features, '--edges', 'no-build,no-dev,no-proc-macro',
               '--target', host_triple(tools), '--prefix', 'none', '--no-dedupe', '--format', '{p}|{l}']
    members = {package['name'] for package in metadata['packages']}
    crates = {}
    for line in tools.text(command, cwd=tools.root).splitlines():
        if not line.strip():
            continue
        package, _, expression = line.partition('|')
        name, version = package.split()[:2]
        if name in members:
            continue
        if '(' in package:
            fail(f'error: {package} is not a crates.io crate; the release recipes vendor only registry crates')
        version = version.removeprefix('v')
        # The legacy `MIT/Apache-2.0` form is a disjunction.
        expression = ' OR '.join(part.strip() for part in expression.split('/')) if '/' in expression else expression.strip()
        if not expression:
            fail(f'error: {name} {version} declares no licence')
        directory = f'{name}-{version}'
        root = vendor / directory
        if not (root / 'Cargo.toml').is_file():
            fail(f'error: {root} missing: cargo vendor did not store {name} {version}')
        manifest = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']
        files = sorted(path.name for path in root.iterdir() if path.is_file() and LICENSE_FILE.fullmatch(path.name))
        if manifest.get('license-file') and Path(manifest['license-file']).name not in files:
            files.append(manifest['license-file'])
        notices = []
        for file in files:
            for text in (root / file).read_text(encoding='utf-8', errors='replace').splitlines():
                match = re.match(r'\s*Copyright\s*(\(c\)|©)?\s*(.*\S)', text)
                if match and (match[1] or re.match(r'\d{4}', match[2])) and match[2] not in notices:
                    notices.append(match[2])
        if not notices:
            authors = manifest.get('authors') or []
            notices = [', '.join(authors)] if authors else [f'the {name} authors']
        crates[directory] = Crate(name, version, expression, directory, files, notices)
    if not crates:
        fail('error: cargo tree listed no linked crate')
    return sorted(crates.values(), key=lambda crate: crate.directory)


def license_ids(expression):
    return [word for word in re.split(r'[\s()]+', expression) if word and word not in ('AND', 'OR', 'WITH')]


def debian_copyright(crates, tools):
    """debian/copyright: the project, the verbatim pin headers and each linked crate."""
    from . import packaging_templates as templates
    paragraphs = [expand(templates.COPYRIGHT_HEADER, dict(HOMEPAGE=HOMEPAGE)).rstrip('\n')]
    # The shipped headers are the pin's, verbatim, and keep their upstream notices.
    notices = {}
    for library in LIBRARIES.values():
        crate = crate_dir(tools.root, library)
        for header in sorted(crate.glob('*.h')):
            if header.is_symlink():
                continue
            found = re.findall(r'Copyright \(C\) (.+?)\s*$', header.read_text(encoding='utf-8'), re.M)
            if not found:
                fail(f'error: {header} carries no copyright line')
            notices.setdefault(found[0], []).append(header.relative_to(tools.root).as_posix())
    for notice, paths in notices.items():
        paragraphs.append(f'Files: {" ".join(paths)}\nCopyright: {notice}\nLicense: GPL-3.0-or-later')

    def debian(expression):
        words = re.split(r'(\s+|[()])', expression)
        names = {'MIT': 'Expat', 'OR': 'or', 'AND': 'and', 'WITH': 'with'}
        return ''.join(names.get(word, word) for word in words)

    used = {'GPL-3.0-or-later'}
    for crate in crates:
        used.update(license_ids(crate.license))
        copyright_lines = '\n '.join(crate.notices)
        paragraphs.append(f'Files: vendor/{crate.directory}/*\nCopyright: {copyright_lines}\n'
                          f'License: {debian(crate.license)}')
    for identifier in sorted(used):
        name = debian(identifier)
        if identifier == 'GPL-3.0-or-later':
            body = (' On Debian systems, the complete text of the GNU General Public\n'
                    ' License version 3 can be found in "/usr/share/common-licenses/GPL-3".')
        elif identifier in templates.COMMON_LICENSES:
            body = (' On Debian systems, the complete text of this licence can be found in\n'
                    f' "/usr/share/common-licenses/{templates.COMMON_LICENSES[identifier]}".')
        elif identifier in templates.LICENSE_TEXTS:
            import textwrap
            body = '\n .\n'.join(textwrap.fill(paragraph, 78, initial_indent=' ', subsequent_indent=' ')
                                 for paragraph in templates.LICENSE_TEXTS[identifier].split('\n\n'))
        else:
            fail(f'error: no licence text for {identifier}; add it to LICENSE_TEXTS in tools/dev/packaging_templates.py')
        paragraphs.append(f'License: {name}\n{body}')
    return '\n\n'.join(paragraphs) + '\n'


def combined_license(crates):
    """The SPDX expression of everything the libraries link: ours AND each crate's."""
    expressions = sorted({crate.license for crate in crates} - {'GPL-3.0-or-later'})
    return ' AND '.join(['GPL-3.0-or-later', *(f'({e})' if ' OR ' in e else e for e in expressions)])


def source_archive(archive, topdir, vendor, tools):
    """The committed tree, the vendored crates and their source replacement, under TOPDIR/."""
    import io
    import tarfile
    from . import packaging_templates as templates
    changed = tools.text(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=tools.root)
    if changed:
        fail('error: the release recipe archives HEAD; commit or discard these changes first:\n' + changed)
    with tempfile.TemporaryDirectory() as scratch:
        tree = Path(scratch) / 'tree.tar'
        tools.run(['git', 'archive', '--format=tar', f'--prefix={topdir}/', '-o', tree, 'HEAD'], cwd=tools.root)

        def owned(info):
            info.uid = info.gid = 0
            info.uname = info.gname = 'root'
            return info

        with tarfile.open(tree) as source, tarfile.open(archive, 'w:gz') as target:
            for member in source:
                target.addfile(member, source.extractfile(member) if member.isfile() else None)
            target.add(vendor, f'{topdir}/vendor', filter=owned)
            config = templates.CARGO_CONFIG.encode()
            info = owned(tarfile.TarInfo(f'{topdir}/.cargo/config.toml'))
            info.size, info.mode = len(config), 0o644
            target.addfile(info, io.BytesIO(config))


def sha256(path):
    import hashlib
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def license_installs(crates, destination, indent=''):
    """The shell lines that install each linked crate's licence files into DESTINATION/<crate>-<version>/."""
    lines = []
    for crate in crates:
        if not crate.files:
            fail(f'error: {crate.name} {crate.version} ships no licence file for its {crate.license} licence')
        paths = ' '.join(f"'vendor/{crate.directory}/{file}'" for file in crate.files)
        lines.append(f'{indent}install -Dm644 -t "{destination}/{crate.directory}" {paths}')
    return '\n'.join(lines)


def release_recipe(argv, tools):
    """Write one distribution's recipe and its source; nothing is built.

    The source holds HEAD, the vendored crates (`cargo vendor`, which needs the network) and
    a source replacement, so the build that follows runs offline. Fedora and Arch keep it as
    an archive; Debian gets the extracted tree, with debian/ in it. The distribution's own
    driver then builds it: dpkg-buildpackage, rpmbuild or makepkg.
    """
    import time
    from . import packaging_templates as templates
    usage = ('usage: release-recipe {debian,fedora,arch} BACKEND VERSION OUTDIR '
             '[--data=PACKAGE --data-version=VER]  (--data: Arch only)')
    if len(argv) < 4 or argv[0] not in BACKEND_BUILD_DEPS:
        fail(usage, 2)
    distro, backend, release, outdir = argv[:4]
    data, data_version = '', ''
    for word in argv[4:]:
        if word.startswith('--data='):
            data = word.partition('=')[2]
        elif word.startswith('--data-version='):
            data_version = word.partition('=')[2]
        else:
            fail(f"error: unknown argument '{word}'\n{usage}", 2)
    if backend not in BACKEND_BUILD_DEPS[distro]:
        fail(f"error: no {distro} recipe for the '{backend}' backend", 2)
    if (distro == 'arch') != bool(data) or (data_version and not data):
        fail('error: --data (the downloaded Arch libpinyin package) is required on Arch and only there', 2)
    data_path = Path(data)
    if data and not data_path.is_file():
        fail(f'error: --data={data} is not a file')
    outdir = Path(outdir).absolute()
    if outdir.exists() and any(outdir.iterdir()):
        fail(f'error: {outdir} is not empty', 2)
    outdir.mkdir(parents=True, exist_ok=True)
    pcver = version(tools.root)
    problems = version_problems(tools.root)
    if problems:
        fail('\n'.join(problems))
    companion_headers(root=tools.root)
    stamp = int(tools.text(['git', 'show', '-s', '--format=%ct', 'HEAD'], cwd=tools.root))
    topdir = f'oxpinyin-{release}'
    with tempfile.TemporaryDirectory() as scratch:
        vendor = Path(scratch) / 'vendor'
        # Its stdout is a source replacement for this scratch path; the archive carries its own.
        done = tools.capture(['cargo', 'vendor', '--quiet', '--locked', '--versioned-dirs', vendor], cwd=tools.root)
        write(done.stderr, error=True)
        if done.status:
            fail('error: cargo vendor failed', done.status)
        crates = shipped_crates(backend, vendor, tools)
        archive = outdir / f'{topdir}.tar.gz'
        source_archive(archive, topdir, vendor, tools)
    major, minor, micro = pcver.split('.')
    fields = dict(BACKEND=backend, VERSION=release, PCVER=pcver, NEXTPCVER=f'{major}.{minor}.{int(micro) + 1}',
                  MAINTAINER=MAINTAINER, HOMEPAGE=HOMEPAGE, BUILD_DEPS=BACKEND_BUILD_DEPS[distro][backend],
                  TOPDIR=topdir, SOURCE=archive.name)
    if distro == 'debian':
        import tarfile
        with tarfile.open(archive) as source:
            source.extractall(outdir, filter='tar')
        archive.unlink()
        tree = outdir / topdir
        fields.update(BASE='oxpinyin-libpinyin15-' + backend, DEBVER=release + '-1',
                      DATE=time.strftime('%a, %d %b %Y %H:%M:%S +0000', time.gmtime(stamp)))
        base = fields['BASE']
        files = {'control': templates.DEB_CONTROL, 'rules': templates.DEB_RULES,
                 'changelog': templates.DEB_CHANGELOG, f'{base}.install': templates.DEB_RUNTIME_INSTALL,
                 f'{base}-dev.install': templates.DEB_DEV_INSTALL, 'not-installed': templates.DEB_NOT_INSTALLED}
        (tree / 'debian/source').mkdir(parents=True)
        for name, template in files.items():
            (tree / 'debian' / name).write_text(expand(template, fields), encoding='utf-8')
        (tree / 'debian/rules').chmod(0o755)
        (tree / 'debian/source/format').write_text('3.0 (quilt)\n', encoding='utf-8')
        (tree / 'debian/copyright').write_text(debian_copyright(crates, tools), encoding='utf-8')
        recipe = tree / 'debian'
    elif distro == 'fedora':
        name = 'oxpinyin-libpinyin-' + backend
        for folder in ('SOURCES', 'SPECS'):
            (outdir / folder).mkdir()
        archive = archive.replace(outdir / 'SOURCES' / archive.name)
        # The breakdown in %cargo_license's form, one linked crate per line.
        (outdir / 'SOURCES/LICENSE.dependencies').write_text(
            ''.join(f'{crate.license}: {crate.name} {crate.version}\n' for crate in crates), encoding='utf-8')

        def rpm_version(semver):
            # cargo2rpm's semver2rpm: a pre-release sorts before its release; build metadata drops.
            return semver.partition('+')[0].replace('-', '~', 1)

        bundled = '\n'.join(f'Provides:       bundled(crate({crate.name})) = {rpm_version(crate.version)}'
                            for crate in crates)
        fields.update(NAME=name, LICENSE=combined_license(crates), BUNDLED=bundled,
                      LICENSE_INSTALL=license_installs(crates, 'crate-licenses'),
                      DATE=time.strftime('%a %b %d %Y', time.gmtime(stamp)))
        recipe = outdir / 'SPECS' / f'{name}.spec'
        recipe.write_text(expand(templates.RPM_SPEC, fields), encoding='utf-8')
    else:
        shutil.copyfile(data_path, outdir / data_path.name)
        description = f'Library to deal with pinyin — oxpinyin drop-in ({backend} store backend'
        if data_version:
            description += '; model data from Arch libpinyin ' + data_version
        description += ')'
        installs = license_installs(crates, '$pkgdir/usr/share/licenses/$pkgname', indent='  ')
        licenses = ['GPL-3.0-or-later', *sorted({crate.license for crate in crates} - {'GPL-3.0-or-later'})]
        fields.update(PKGNAME='oxpinyin-libpinyin-' + backend, DESC=description, ARCH=tools.text(['uname', '-m']),
                      EXTRA_DEPS=BACKEND_BUILD_DEPS[distro][backend], LICENSES=' '.join(f"'{item}'" for item in licenses),
                      DATA=data_path.name, SOURCE_SUM=sha256(archive), DATA_SUM=sha256(outdir / data_path.name),
                      LICENSE_INSTALL=installs)
        recipe = outdir / 'PKGBUILD'
        recipe.write_text(expand(templates.ARCH_BUILD, fields), encoding='utf-8')
    emit(f'== {distro} recipe for {backend}: {recipe} ==')
    emit(f'linked crates ({len(crates)}): ' + ', '.join(f'{crate.name} {crate.version} ({crate.license})' for crate in crates))
    return 0


def check_installed(argv, tools):
    """Check the installed libraries under LIBDIR, then certify the libtkrzw each program loads.

    The exports must be the .ver script's, each under its default version (@@ and the node),
    the SONAME the pin's, and the binary version of LIBDIR/pkgconfig/<library>.pc 15.0. The
    packages' builds ran the same export check. On the tkrzw backend each program must load
    the certified libtkrzw.
    """
    from tools.tkrzw.runtime import RuntimeRefused, check
    if len(argv) < 2 or argv[0] not in cell.BACKENDS:
        fail('usage: check-installed BACKEND LIBDIR [PROGRAM ...]', 2)
    backend, libdir, programs = argv[0], Path(argv[1]), argv[2:]
    problems = []
    for library, name in LIBRARIES.items():
        path = libdir / f'{library}.so.15.0.0'
        if not path.is_file():
            fail(f'error: {path} is not installed')
        names = re.findall(r'\(SONAME\).*\[([^]]*)\]', tools.text(['readelf', '-d', path]))
        if names != [library + '.so.15']:
            problems.append(f'error: {path} SONAME is {names}, want {library}.so.15')
        node, expected = version_exports(crate_dir(tools.root, name) / (library + '.ver'))
        raw = tools.text(['nm', '-D', '--defined-only', path])
        actual = sorted(linux_exports(raw, node))
        if actual != expected:
            problems.append(f'error: {path} exports differ from {library}.ver: '
                            f'missing {sorted(set(expected) - set(actual))}, extra {sorted(set(actual) - set(expected))}')
        unversioned = [symbol for symbol in linux_symbol_names(raw, node) if not symbol.endswith('@@' + node)]
        if unversioned:
            problems.append(f'error: {path} exports symbols without the default version @@{node}: {unversioned}')
        # The installed file itself, not whichever module of that name pkg-config finds first.
        pc = libdir / 'pkgconfig' / f'{library}.pc'
        if not pc.is_file():
            fail(f'error: {pc} is not installed')
        variable = tools.text(['pkg-config', f'--variable={library}_binary_version', pc])
        if variable != BINARY_VERSION:
            problems.append(f"error: {pc} {library}_binary_version is '{variable}', want '{BINARY_VERSION}'")
        emit(f'{path}: SONAME {library}.so.15, {len(actual)} exports under {node}')
    for program in programs:
        try:
            loaded = check(backend, 'just packaging check-installed', 'subject', program)
        except RuntimeRefused as error:
            problems.append('REFUSED: ' + str(error))
        else:
            if loaded:
                emit(f'{program}: libtkrzw {loaded}')
            elif backend == 'tkrzw':
                problems.append(f'error: {program} loads no libtkrzw on the tkrzw backend')
            else:
                emit(f'{program}: loads no libtkrzw')
    if problems:
        fail('\n'.join(problems))
    return 0


def metadata(argv, tools):
    from . import packaging_templates as templates
    values = dict(pin_root='', subject_root='', prefix='', libdir='', includedir='',
                  build_subject='', modules='libpinyin libzhuyin')
    for word in argv:
        key, separator, value = word.partition('=')
        name = key.removeprefix('--').replace('-', '_')
        if not key.startswith('--') or not separator or name not in values:
            emit(f"error: unknown argument '{word}'", error=True)
            fail(templates.METADATA_USAGE.rstrip('\n'), 2)
        values[name] = value
    if not all(values[name] for name in ('pin_root', 'subject_root', 'prefix')):
        fail(templates.METADATA_USAGE.rstrip('\n'), 2)
    if not os.path.isabs(values['subject_root']):
        fail(f"error: --subject-root '{values['subject_root']}' must be an absolute path", 2)
    if not tools.which('pkg-config'):
        fail('error: pkg-config not found', 2)
    from .cell import Settings
    backend = values['build_subject']
    environment = dict(tools.env)
    if backend in cell.BACKENDS:
        environment.setdefault('PINYIN_ORACLE_DBM', 'kc' if backend == 'kyotocabinet' else backend)
    try:
        settings = Settings.read(environment, tools.root)
    except OSError as error:
        fail(f'FAIL: {error}')
    for key in ('ORACLE_DBM', 'ORACLE_DBM_NAME', 'CAPI_FEATURE', 'OXPINYIN_CAPI_BACKEND_EXT',
                'EXPECTED_PIN_REF', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL',
                'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO'):
        if key in settings.environment:
            tools.env[key] = settings.environment[key]
    with chdir(tools.root):
        if backend:
            tools.run(['rm', '-rf', '--', values['subject_root']])
            layout = [f'--prefix={values["prefix"]}', f'--destdir={values["subject_root"]}']
            layout += [f'--{key}={values[key]}' for key in ('libdir', 'includedir') if values[key]]
            for library in LIBRARIES:
                install([library, *layout, '--', '--profile', 'dev', '--locked', '--no-default-features',
                         '--features', backend], tools)
        roots = []
        for key, label in [('pin_root', 'pin'), ('subject_root', 'subject')]:
            if not Path(values[key]).is_dir():
                fail(f'error: no {label} root', 2)
            roots.append(str(Path(values[key]).resolve()))
        prefix = values['prefix']
        libdir, includedir = values['libdir'] or prefix + '/lib', values['includedir'] or prefix + '/include'
        relocated, differences = '/relocated-by-check-pc-metadata', 0
        with temporary_directory(tools) as work:
            def compare(label, texts):
                nonlocal differences
                paths = [Path(work) / 'pin', Path(work) / 'subject']
                for path, text in zip(paths, texts, strict=True):
                    path.write_bytes(text)
                done = tools.capture(['diff', '-u', '--label', 'pin: ' + label, '--label', 'oxpinyin: ' + label, '--', *paths])
                write(done.stdout)
                write(done.stderr, error=True)
                differences += bool(done.status)

            def pc(root, *arguments):
                env = {**tools.env, 'PKG_CONFIG_PATH': root + libdir + '/pkgconfig'}
                done = tools.capture(['pkg-config', *arguments], env=env, merge_stderr=True)
                out = done.stdout.decode(errors='surrogateescape').rstrip('\n').replace(root, '<root>')
                return f'{out}\nexit={done.status}\n'

            emit('== include directory ' + includedir)
            headers = []
            for root in roots:
                directory_path = Path(root + includedir)
                if directory_path.is_dir():
                    listing = tools.text(['find', '.', '-mindepth', '1'], cwd=directory_path)
                    headers.append(('\n'.join(sorted(listing.splitlines())) + ('\n' if listing else '')).encode())
                else:
                    headers.append(f'(no {includedir})\n'.encode())
            compare('headers under ' + includedir, headers)
            for module in values['modules'].split():
                emit('== ' + module)
                relative = libdir + '/pkgconfig/' + module + '.pc'
                compare(relative, [Path(root + relative).read_bytes() if Path(root + relative).is_file()
                                   else f'(missing {relative})\n'.encode() for root in roots])
                variables = sorted({line for root in roots for line in pc(root, '--print-variables', module).splitlines()
                                    if line and not line.startswith('exit=')})
                env = {**tools.env, 'PKG_CONFIG_PATH': roots[0] + libdir + '/pkgconfig'}
                done = tools.capture(['pkg-config', '--modversion', module], env=env)
                write(done.stderr, error=True)
                if done.status:
                    fail(f'error: pkg-config cannot resolve {module} in the pin tree ({roots[0]}{libdir}/pkgconfig)', 2)
                pin_version = done.stdout.decode().rstrip('\n')
                queries = []
                for root in roots:
                    text = ''
                    for query in ('--modversion', '--cflags', '--libs', '--print-variables'):
                        text += f'## pkg-config {query}\n' + pc(root, query, module)
                    for variable in variables:
                        text += f'## pkg-config --variable={variable}\n' + pc(root, '--variable=' + variable, module)
                    text += f"## pkg-config --atleast-version={pin_version} (the pin's modversion)\n" + pc(root, '--atleast-version=' + pin_version, module)
                    text += f'## pkg-config --exact-version={pin_version}\n' + pc(root, '--exact-version=' + pin_version, module)
                    for query in ('--cflags', '--libs'):
                        text += f'## pkg-config --define-variable=prefix={relocated} {query}\n' + pc(root, '--define-variable=prefix=' + relocated, query, module)
                    for variable in variables:
                        text += f'## pkg-config --define-variable=prefix={relocated} --variable={variable}\n' + pc(root, '--define-variable=prefix=' + relocated, '--variable=' + variable, module)
                    queries.append(text.encode())
                compare('pkg-config queries for ' + module, queries)
        suffix = f'(prefix={prefix} libdir={libdir} includedir={includedir})'
        if differences:
            fail(f'FAIL: {differences} item(s) differ from the pin {suffix}')
        emit('OK: packaging metadata matches the pin ' + suffix)
    return 0


def sizes(argv, tools):
    """Base and head stripped sizes; orchestration lives in ship_sizes."""
    from . import ship_sizes
    return ship_sizes.main(argv)


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    tools = Tools()
    if not argv or argv[0] not in NAMES:
        emit('usage: python -m tools.dev.packaging {' + ','.join(NAMES) + '} ...', error=True)
        return 2
    name, rest = argv[0], argv[1:]
    functions = {'install': install,
                 'trainer-layout': trainer_layout,
                 'check-pc-version': check_version,
                 'check-pc-metadata': metadata,
                 'distro-tkrzw-library': distro_tkrzw_library,
                 'check-library-mode': check_library_mode,
                 'check-installed': check_installed,
                 'release-version': release_version, 'release-recipe': release_recipe, 'sizes': sizes}
    def call():
        if name == 'pc-version':
            emit(version(tools.root))
            return 0
        return functions[name](rest, tools)
    def quiet_cancellation():
        try:
            result = call()
            slot.check_cancelled()
            return result
        except (pool.UnitStopped, pool.UnitsCancelled, slot.Cancelled):
            raise Finished(slot.cancellation_status()) from None
    return guarded(quiet_cancellation)


if __name__ == '__main__':
    raise SystemExit(main())
