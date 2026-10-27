"""Install, validate and assemble the drop-in packages (#1292).

One layout and process adapter serve every entry point. Policy identities and
resolution remain in tools.packaging.function_order and its existing AWK program.
Cargo and package builders use the shared heavy admission; captured tools use
pool.UnitContext, which reaps their process groups on cancellation.
"""
from contextlib import contextmanager
from dataclasses import dataclass
import io
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile

from . import pool, slot
from .runners import Finished, emit, guarded, status, write
from tools.packaging import function_order

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tools/packaging'
LIBRARIES = {'libpinyin': 'oxpinyin-capi', 'libzhuyin': 'oxpinyin-zhuyin-capi'}
HEADERS = ('novel_types.h', 'pinyin_custom2.h')
NAMES = ('install', 'relink-versioned', 'pc-version', 'check-pc-version', 'check-pc-metadata',
         'function-order', 'release-stage', 'release-deb', 'release-rpm', 'release-arch')


def fail(message='', code=1):
    if message:
        emit(message, error=True)
    raise Finished(code)


@contextmanager
def directory(path):
    previous = Path.cwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)


class Tools:
    """Process boundary, injectable for fixtures without replacing global services."""
    def __init__(self, env=None, *, root=ROOT):
        self.root = Path(root)
        self.here = self.root / "tools/packaging"
        self.env = dict(os.environ if env is None else env)

    def capture(self, argv, *, cwd=None, env=None, merge_stderr=False):
        context = pool.UnitContext('packaging', slot.CANCELLED,
            environment=dict(self.env if env is None else env),
            fds=slot.inherited_fds('packaging', out=io.StringIO()))
        done = context.launch(list(map(str, argv)), cwd=cwd, merge_stderr=merge_stderr)
        return done._replace(status=status(done.status))

    def run(self, argv, **options):
        done = self.capture(argv, **options)
        write(done.stdout)
        write(done.stderr, error=True)
        if done.status:
            raise Finished(done.status)
        return done.stdout.decode(errors='surrogateescape').rstrip('\n')

    def heavy(self, argv, *, cwd=None, env=None):
        with directory(cwd or Path.cwd()):
            code = status(slot.run_command(list(map(str, argv)), env=self.env if env is None else env))
        if code:
            raise Finished(code)

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
    values = re.findall(r'^libpinyin_tag=(.*)$', record.read_text(), re.M)
    value = '\n'.join(values)
    if not re.search(r'^[0-9]+\.[0-9]+\.[0-9]+$', value, re.M):
        fail(f'error: {record}: no well-formed libpinyin_tag=<major>.<minor>.<micro> line')
    return value


def companion_headers(*, stage=False, root=ROOT):
    for name in HEADERS:
        paths = [root / 'crates' / crate / name for crate in LIBRARIES.values()]
        try:
            same = paths[0].read_bytes() == paths[1].read_bytes()
        except OSError:
            same = False
        if not same:
            message = f"error: companion header '{name}' differs between the two crates"
            if not stage:
                message += f':\n         {paths[0]}\n         {paths[1]}\n'
                message += '       both ship into the same include subdirectory and must be byte-identical.'
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
        usage = ('usage: tools/packaging/install.sh <libpinyin|libzhuyin> --prefix=DIR '
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
        with directory(tools.here):
            target_path = Path(target) if target else tools.root / 'target'
            target_path.mkdir(parents=True, exist_ok=True)
            target_path = target_path.resolve()
        triple = triple or tools.env.get('CARGO_BUILD_TARGET') or ''
        if not triple:
            triple = '\n'.join(re.findall(r'^host: (.*)$', tools.text(['rustc', '-vV']), re.M))
        triple = os.path.basename(triple).removesuffix('.json')
        if not triple:
            fail("error: could not determine the build target (rustc -vV had no 'host:' line)")
        return cls(library, prefix, values['libdir'] or prefix + '/lib',
                   values['includedir'] or prefix + '/include', values['destdir'],
                   values['libdir'] or '${exec_prefix}/lib', values['includedir'] or '${prefix}/include',
                   target_path, triple, profile, passthru)

    def baked(self):
        name = self.library + '.pc.in.baked'
        mirror = self.qualified / name
        if mirror.is_file():
            return mirror
        candidates = [path for path in (self.qualified / 'build').glob('*/out/' + name) if path.is_file()]
        if len(candidates) == 1:
            return candidates[0]
        if candidates:
            fail(f"error: multiple '{name}' pkg-config templates under {self.qualified}/build:\n" +
                 ''.join(f'         {path}\n' for path in candidates) +
                 "       remove the stale build dirs (or 'cargo clean') and retry.")
        fail(f'error: baked pkg-config template not found for this build:\n'
             f'         {mirror}\n         (nor a unique {self.qualified}/build/*/out/{name})\n'
             f"       (target='{self.triple}' profile='{self.profile}' target-dir='{self.target}')\n"
             "       build.rs writes it during 'cargo cinstall'; check the passthrough args match the build.")

    def features(self):
        result, index = '', 0
        while index < len(self.passthru):
            if self.passthru[index].split('=')[0] == '--features':
                _, value, index = option(self.passthru, index)
                result = (result + ',' if result else '') + value
            else:
                index += 1
        return result


def install(argv, tools):
    layout = Layout.parse(argv, tools)
    companion_headers(root=tools.root)
    layout.pcdir.mkdir(parents=True, exist_ok=True)
    crate = tools.root / 'crates' / LIBRARIES[layout.library]
    command = ['cargo', 'cinstall', f'--prefix={layout.prefix}', f'--libdir={layout.libdir}',
               f'--includedir={layout.includedir}', f'--target-dir={layout.target}']
    if layout.destdir:
        command.append(f'--destdir={layout.destdir}')
    tools.heavy([*command, *layout.passthru], cwd=crate)
    data = layout.baked().read_bytes()
    for name, value in [('prefix', layout.prefix), ('exec_prefix', '${prefix}'),
                        ('libdir', layout.pc_libdir), ('includedir', layout.pc_includedir)]:
        data = data.replace(f'@{name}@'.encode(), os.fsencode(value))
    pc = layout.pcdir / (layout.library + '.pc')
    pc.write_bytes(data)
    emit(f'installed complete pkg-config file: {pc}')
    if tools.text(['uname', '-s']) == 'Linux':
        with directory(tools.here):
            relink(['--staticlib', str(layout.qualified / (layout.library + '.a')),
                    '--ver', str(crate / (layout.library + '.ver')),
                    '--soname', layout.library + '.so.15',
                    '--dest', layout.destdir + layout.libdir + '/' + layout.library + '.so.15.0.0',
                    *(['--features', layout.features()] if layout.features() else [])], tools)
    return 0


def backend_flags(backend, tools):
    if backend == 'bdb':
        return ['-ldb']
    done = tools.capture(['pkg-config', '--libs', backend])
    flags = done.stdout.decode().split() if not done.status else ['-l' + backend]
    return [*flags, *(['-lstdc++'] if backend == 'kyotocabinet' else [])]


def relink(argv, tools):
    values = dict(staticlib='', ver='', soname='', dest='', features=tools.env.get('OXPINYIN_RELINK_FEATURES') or '',
                  cc=tools.env.get('CC') or 'cc', function_order=tools.env.get('OXPINYIN_FUNCTION_ORDER_POLICY', ''))
    selected = 'OXPINYIN_FUNCTION_ORDER_POLICY' in tools.env
    index = 0
    usage = ('usage: tools/packaging/relink-versioned.sh --staticlib PATH --ver PATH --soname NAME '
             '--dest PATH [--features LIST] [--cc CC] [--function-order POLICY]')
    while index < len(argv):
        key, value, index = option(argv, index)
        name = key.removeprefix('--').replace('-', '_')
        if not key.startswith('--') or name not in values:
            fail(usage, 2)
        values[name] = value
        selected |= name == 'function_order'
    archive, ver, soname, dest = (values[key] for key in ('staticlib', 'ver', 'soname', 'dest'))
    if not all((archive, ver, soname, dest)):
        fail(usage, 2)
    if tools.text(['uname', '-s']) != 'Linux':
        fail('error: relink-versioned.sh is Linux-only (ELF symbol versioning); run it in the Linux packaging lane')
    cc = values['cc']
    for tool in (cc, 'nm', 'readelf'):
        if not tools.which(tool):
            fail(f"error: required tool '{tool}' not found")
    for path, label in ((archive, 'staticlib'), (ver, 'version script'), (dest, 'installed object')):
        if not Path(path).is_file():
            fail(f'error: {label} not found: {path}')
    backend = 'bdb'
    for feature in values['features'].replace(',', ' ').split():
        if feature in ('tkrzw', 'kyotocabinet', 'bdb'):
            backend = feature
    glib = tools.capture(['pkg-config', '--libs', 'glib-2.0', 'gobject-2.0'])
    flags = glib.stdout.decode().split() if not glib.status else ['-lglib-2.0', '-lgobject-2.0']
    flags += backend_flags(backend, tools)
    policy_backend = 'kc' if backend == 'kyotocabinet' else backend
    policy = values['function_order']
    source = ('named' if policy else 'opt-out') if selected else 'checked-in'
    if not selected:
        policy = str(tools.here / 'function-order' / policy_backend / (soname.split('.so')[0] + '.order'))
    destination = Path(dest)
    # As mktemp ${DEST%.so*}.relink.XXXXXX: scratch stays beside the object,
    # so the verified replacement is atomic and its symlinks keep resolving.
    prefix = destination.name.rsplit('.so', 1)[0] + '.relink.'
    fd, temporary = tempfile.mkstemp(prefix=prefix, dir=destination.parent)
    os.close(fd)
    try:
        order = []
        if source == 'opt-out':
            emit('function ordering: disabled by explicit opt-out; using the original layout', error=True)
        else:
            if not Path(policy).is_file():
                fail(f'error: function ordering: policy not found: {policy} ({source})')
            try:
                function_order.check_policy_backend(Path(policy), policy_backend)
            except ValueError as error:
                fail('error: function ordering: ' + str(error))
            with tempfile.TemporaryDirectory(prefix=Path(temporary).name + '.order.',
                                             dir=destination.parent) as scratch:
                done = function_order.resolve('resolve', archive, policy, soname,
                    run=lambda command: tools.capture(command, env={**tools.env, 'LC_ALL': 'C'}))
                write(done.stderr, error=True)
                order_file = Path(scratch) / 'order.ld'
                order_file.write_bytes(done.stdout)
                if done.status:
                    fail(f'error: function ordering: policy {policy} does not resolve against {archive}; '
                         'regenerate it (docs/packaging.md, "Regeneration") or disable ordering explicitly '
                         "with --function-order ''")
                if function_order.supported(cc, archive, scratch,
                        run=lambda command: tools.capture(command, env={**tools.env, 'LC_ALL': 'C'})):
                    order = ['-Xlinker', '--section-ordering-file', '-Xlinker', str(order_file)]
                    emit(f'function ordering: using measured policy {policy}', error=True)
                else:
                    emit('function ordering: the linker does not support --section-ordering-file; using the original layout', error=True)
                link_and_verify(cc, temporary, order, ver, soname, archive, flags, tools)
        if source == 'opt-out':
            link_and_verify(cc, temporary, order, ver, soname, archive, flags, tools)
        # mv -f replaces a symlink at DEST, rather than writing through it.
        os.replace(temporary, dest)
    finally:
        Path(temporary).unlink(missing_ok=True)
    node, exports = version_exports(ver)
    emit(f'relinked {dest}')
    emit(f'  soname: {soname} · version node: {node} · exports: {len(exports)} versioned (backend: {backend})')
    return 0


def version_exports(ver):
    text = Path(ver).read_text()
    nodes = re.findall(r'^([A-Za-z_][A-Za-z0-9_]*) \{.*', text, re.M)
    if not nodes:
        fail(f'error: no version node found in {ver}')
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


def link_and_verify(cc, temporary, order, ver, soname, archive, flags, tools):
    tools.heavy([cc, '-shared', '-o', temporary, *order, '-Wl,-z,defs', f'-Wl,-soname,{soname}',
                 f'-Wl,--version-script={ver}', '-Wl,--gc-sections', '-Wl,--whole-archive', archive,
                 '-Wl,--no-whole-archive', *flags, '-lpthread', '-ldl', '-lm'])
    node, expected = version_exports(ver)
    raw = tools.text(['nm', '-D', '--defined-only', temporary])
    symbols = [line.split()[-1] for line in raw.splitlines() if line.split()]
    actual = sorted(name.split('@')[0] for name in symbols if name.split('@')[0] != node)
    if expected != actual:
        emit(f"error: relinked object's export set differs from {ver}:", error=True)
        with tempfile.TemporaryDirectory() as scratch:
            left, right = Path(scratch) / 'ver', Path(scratch) / 'so'
            left.write_text(''.join(name + '\n' for name in expected))
            right.write_text(''.join(name + '\n' for name in actual))
            done = tools.capture(['diff', '-u', left, right])
            write(b'\n'.join(done.stdout.splitlines()[2:40]) + b'\n', error=True)
        raise Finished(1)
    # Preserve the second nm validation: a failing inspection is a failed run.
    unversioned = [line.split()[-1] for line in tools.text(['nm', '-D', '--defined-only', temporary]).splitlines()
                   if line.split() and '@' not in line.split()[-1] and line.split()[-1] != node]
    if unversioned:
        fail('error: exported symbols without a version tag:\n' + '\n'.join('  ' + name for name in unversioned))
    names = re.findall(r'\(SONAME\).*\[([^]]*)\]', tools.text(['readelf', '-d', temporary]))
    actual_soname = names[0] if names else ''
    if actual_soname != soname:
        fail(f"error: relinked SONAME is '{actual_soname or '<none>'}', expected {soname}")


def check_version(argv, tools):
    want, bad = version(tools.root), False
    for crate in LIBRARIES.values():
        path = tools.root / 'crates' / crate / 'Cargo.toml'
        text = '\n'.join(line for line in path.read_text().splitlines() if not re.match(r'^\s*#', line))
        for value in re.findall(r'"(libpinyin-[^"]*)"', text):
            if value != 'libpinyin-' + want:
                emit(f'error: {path} declares "{value}", want "libpinyin-{want}"', error=True)
                bad = True
        if f'"libpinyin-{want}"' not in text:
            emit(f'error: {path} declares no "libpinyin-{want}" include subdirectory', error=True)
            bad = True
        section = ''
        versions = []
        for line in text.splitlines():
            if line.startswith('['):
                section = line
            if section == '[package.metadata.capi.pkg_config]' and re.match(r'^version\s*=', line):
                versions.append(re.sub(r'.*=\s*"|".*', '', line))
        got = '\n'.join(versions)
        if got != want:
            emit(f'error: {path} [package.metadata.capi.pkg_config] version is "{got}", want "{want}"', error=True)
            bad = True
    for library, crate in LIBRARIES.items():
        path = tools.root / 'crates' / crate / (library + '.pc.in')
        text = '\n'.join(line for line in path.read_text().splitlines() if not line.startswith('#'))
        if re.search(r'[0-9]+\.[0-9]+\.[0-9]+', text):
            emit(f'error: {path} carries a literal version; it must use @VERSION@', error=True)
            bad = True
    if bad:
        return 1
    emit(f'OK: every drop-in version declaration is {want} (tools/oracle/oracle-pin.txt)')
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


def package_work(outdir):
    Path(outdir).mkdir(parents=True, exist_ok=True)
    return tempfile.TemporaryDirectory()


def package_done(kind, outdir, tools):
    emit(f'== {kind} written to {outdir} ==')
    tools.run(['ls', '-l', outdir])
    return 0


def release_deb(argv, tools):
    from . import packaging_templates as templates
    backend, release, stage, libdir, outdir = positional(argv,
        ('backend', 'version', 'stage root', 'libdir', 'outdir'))
    arch = tools.text(['dpkg', '--print-architecture'])
    pcver = version(tools.root)
    stage_lib = Path(stage + libdir)
    for name in ('libpinyin.so.15.0.0', 'libzhuyin.so.15.0.0'):
        if not (stage_lib / name).is_file():
            fail(f'error: {stage_lib / name} missing')
    Path(outdir).mkdir(parents=True, exist_ok=True)
    libraries = [str(stage_lib / (lib + '.so.15.0.0')) for lib in LIBRARIES]
    sonames = sorted(set(re.findall(r'^\s*NEEDED\s+(\S+)', tools.text(['objdump', '-p', *libraries]), re.M)))
    packages = set()
    for soname in sonames:
        if soname in ('libpinyin.so.15', 'libzhuyin.so.15'):
            continue
        cache = tools.text(['ldconfig', '-p'])
        paths = [line.split()[-1] for line in cache.splitlines() if line.split() and line.split()[0] == soname]
        if not paths:
            fail(f'error: soname {soname} not in ldconfig cache')
        path = os.path.realpath(paths[0])
        done = tools.capture(['dpkg', '-S', '--', path])
        package = done.stdout.decode().split('\n')[0].split(':')[0]
        if not package:
            fail(f'error: no package owns {path} ({soname})')
        packages.add(package)
    floors = re.findall(r'GLIBC_([0-9.]*)', tools.text(['objdump', '-T', *libraries]))
    # GNU version ordering, including an absent GLIBC floor, is the old sort -V.
    with tempfile.NamedTemporaryFile() as source:
        source.write(''.join(value + '\n' for value in floors).encode())
        source.flush()
        ordered = tools.text(['sort', '-Vu', source.name]).splitlines()
    deps = 'libc6 (>= ' + (ordered[-1] if ordered else '') + ')'
    deps += ''.join(', ' + package for package in sorted(packages) if package != 'libc6')
    base, debver = 'oxpinyin-libpinyin15-' + backend, release + '-1'
    fields = dict(BASE=base, BACKEND=backend, DEBVER=debver, ARCH=arch, PCVER=pcver, DEPS=deps,
                  MAINTAINER='Shenghao Chen <shenghaoc@outlook.com>', HOMEPAGE='https://github.com/shenghaoc/oxpinyin')
    with package_work(outdir) as work:
        runtime, dev = Path(work) / 'runtime', Path(work) / 'dev'
        rtlib, devlib = Path(str(runtime) + libdir), Path(str(dev) + libdir)
        inc = dev / 'usr/include' / ('libpinyin-' + pcver)
        for path in (rtlib, devlib / 'pkgconfig', inc, runtime / 'DEBIAN', dev / 'DEBIAN',
                     runtime / 'usr/share/doc' / base, dev / 'usr/share/doc' / (base + '-dev')):
            path.mkdir(parents=True, exist_ok=True)
        for library in LIBRARIES:
            tools.run(['install', '-m0755', '--', stage_lib / (library + '.so.15.0.0'), str(rtlib) + '/'])
            tools.run(['ln', '-sfn', '--', library + '.so.15.0.0', rtlib / (library + '.so.15')])
        tools.run(['install', '-m0644', '--', *[stage_lib / (lib + '.a') for lib in LIBRARIES], str(devlib) + '/'])
        tools.run(['install', '-m0644', '--', *[stage_lib / 'pkgconfig' / (lib + '.pc') for lib in LIBRARIES], str(devlib / 'pkgconfig') + '/'])
        headers = sorted((Path(stage) / 'usr/include' / ('libpinyin-' + pcver)).glob('*.h'))
        tools.run(['install', '-m0644', '--', *(headers or [Path(stage) / 'usr/include' / ('libpinyin-' + pcver) / '*.h']), str(inc) + '/'])
        for library in LIBRARIES:
            tools.run(['ln', '-sfn', '--', library + '.so.15', devlib / (library + '.so')])
        for tree, name, control in ((runtime, base, templates.DEB_RUNTIME), (dev, base + '-dev', templates.DEB_DEV)):
            (tree / 'DEBIAN/control').write_text(expand(control, fields))
            copyright_text = expand(templates.COPYRIGHT, fields)
            (tree / 'usr/share/doc' / name / 'copyright').write_text(copyright_text)
            sums = tree / 'DEBIAN/md5sums'
            sums.touch()
            files = sorted(str(path.relative_to(tree)) for path in tree.rglob('*') if path.is_file() and not path.is_symlink())
            sums.write_text(tools.text(['md5sum', *files], cwd=tree) + '\n')
        for tree, name in ((runtime, base), (dev, base + '-dev')):
            tools.heavy(['dpkg-deb', '--root-owner-group', '--build', tree,
                         Path(outdir) / f'{name}_{debver}_{arch}.deb'])
    return package_done('debs', outdir, tools)


def release_rpm(argv, tools):
    from . import packaging_templates as templates
    backend, release, stage, outdir = positional(argv, ('backend', 'version', 'stage root', 'outdir'))
    pcver, name = version(tools.root), 'oxpinyin-libpinyin-' + backend
    if not (Path(stage) / 'usr').is_dir():
        fail(f'error: {stage}/usr missing')
    with package_work(outdir) as work:
        root = Path(work).resolve()
        for folder in ('BUILD', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS'):
            (root / folder).mkdir()
        tools.run(['tar', '-czf', root / 'SOURCES/staged-tree.tar.gz', '-C', stage, 'usr'])
        spec = root / 'SPECS' / (name + '.spec')
        spec.write_text(expand(templates.RPM_SPEC, dict(NAME=name, VERSION=release, BACKEND=backend, PCVER=pcver)))
        tools.heavy(['rpmbuild', '--define', '_topdir ' + str(root), '-bb', spec])
        rpms = sorted((root / 'RPMS').glob('*/*.rpm'))
        if not rpms:
            fail('error: rpmbuild produced no RPMs')
        for rpm in rpms:
            tools.text(['rpm', '-qlp', rpm])
            tools.run(['cp', '-f', '--', rpm, outdir + '/'])
    return package_done('rpms', outdir, tools)


def release_arch(argv, tools):
    from . import packaging_templates as templates
    backend, release, stage, outdir = positional(argv, ('backend', 'version', 'stage root', 'outdir'))
    data, data_version = '', ''
    for word in argv[4:]:
        if word.startswith('--data='):
            data = word.partition('=')[2]
        elif word.startswith('--data-version='):
            data_version = word.partition('=')[2]
        else:
            fail(f"error: unknown argument '{word}'", 2)
    if data:
        if not (Path(data) / 'table.conf').is_file():
            fail(f'error: --data={data} holds no table.conf')
        data = data.removesuffix('/')
    elif data_version:
        fail('error: --data-version given without --data', 2)
    description = f'Library to deal with pinyin — oxpinyin drop-in ({backend} store backend'
    if data_version:
        description += '; model data from Arch libpinyin ' + data_version
    description += ')'
    pcver, name = version(tools.root), 'oxpinyin-libpinyin-' + backend
    arch = tools.text(['uname', '-m'])
    if backend not in ('kyotocabinet', 'tkrzw'):
        fail(f"error: unknown backend '{backend}'", 2)
    if not (Path(stage) / 'usr').is_dir():
        fail(f'error: {stage}/usr missing')
    with package_work(outdir) as work:
        outdir = str(Path(outdir).resolve())
        os.chmod(work, 0o711)
        pkgwork = Path(work) / 'pkg'
        pkgwork.mkdir()
        tools.run(['cp', '-a', '--', Path(stage) / 'usr', pkgwork / 'stage-usr'])
        if data:
            target = pkgwork / 'stage-usr/lib/libpinyin/data'
            if target.exists():
                fail('error: staged tree already carries lib/libpinyin/data')
            target.parent.mkdir(parents=True, exist_ok=True)
            tools.run(['cp', '-a', '--', data, target])
        fields = dict(PKGNAME=name, VERSION=release, DESC=description, ARCH=arch,
                      EXTRA_DEPS=backend, PCVER=pcver, SOARCH=64)
        (pkgwork / 'PKGBUILD').write_text(expand(templates.ARCH_BUILD, fields))
        if tools.capture(['id', '-u', 'oxp-builder']).status:
            tools.run(['useradd', '--create-home', '--shell', '/bin/bash', 'oxp-builder'])
        tools.run(['chown', '-R', '--', 'oxp-builder', pkgwork])
        tools.run(['chmod', 'a+w', '--', outdir])
        # su's external interface takes one shell command. Preserve that
        # interface and its quoting; the package recipe itself is shell data.
        tools.heavy(['su', 'oxp-builder', '-c',
            f"cd '{pkgwork}' && env PKGDEST='{outdir}' makepkg --nodeps --noconfirm --force"])
        package = Path(outdir) / f'{name}-{release}-1-{arch}.pkg.tar.zst'
        if not package.is_file():
            fail(f'error: expected package {package} not found')
    return package_done('arch package', outdir, tools)


def release_stage(argv, tools):
    from . import packaging_templates as templates
    from tools.tkrzw.runtime import RuntimeRefused, check
    pcver = version(tools.root)
    usage = 'usage: tools/packaging/release-stage.sh <kyotocabinet|tkrzw|bdb> --prefix=DIR --libdir=DIR [--dest=DIR]'
    if not argv or argv[0] not in ('kyotocabinet', 'tkrzw', 'bdb'):
        fail(usage, 2)
    backend, values, index = argv[0], dict(prefix='', libdir='', dest=''), 1
    while index < len(argv):
        key, value, index = option(argv, index)
        if key not in ('--prefix', '--libdir', '--dest'):
            fail(usage, 2)
        values[key[2:]] = value
    prefix, libdir = values['prefix'], values['libdir']
    if not prefix or not libdir:
        fail(usage, 2)
    dest = values['dest'] or str(tools.root / 'target/release-stage' / backend)
    if dest in ('/', prefix, libdir):
        fail(f"error: --dest '{dest}' collides with a real install root", 2)
    if not tools.which('strip'):
        fail('error: strip is required; install binutils')
    if not tools.which('cmp'):
        fail('error: cmp (diffutils) is required for the companion-header check')
    companion_headers(stage=True, root=tools.root)
    if not tools.which('cargo-cinstall'):
        fail("error: cargo-c (cargo cinstall) is required; install the distro's cargo-c package")
    with directory(tools.here):
        tools.run(['rm', '-rf', '--', dest])
        for library in LIBRARIES:
            suffix = ', features: shipped' if library == 'libpinyin' else ''
            emit(f'== installing {library} into {dest} (backend: {backend}{suffix}) ==')
            install([library, f'--prefix={prefix}', f'--libdir={libdir}', f'--destdir={dest}', '--',
                     '--release', '--locked', '--no-default-features', '--features',
                     backend + (',shipped' if library == 'libpinyin' else '')], tools)
        lib, inc = Path(dest + libdir), Path(dest + prefix) / 'include' / ('libpinyin-' + pcver)
        pc = lib / 'pkgconfig'
        expected = [lib / (library + extension) for library in LIBRARIES
                    for extension in ('.so.15.0.0', '.so.15', '.so', '.a')]
        expected += [pc / (library + '.pc') for library in LIBRARIES]
        expected += [inc / name for name in ('pinyin.h', 'zhuyin.h', *HEADERS, 'zhuyin_custom2.h')]
        for path in expected:
            if not path.exists():
                fail(f'error: expected staged file {path} is missing')
        unexpected = tools.text(['find', dest, '-type', 'f', '!', '-path', str(lib) + '/*',
                                 '!', '-path', str(inc) + '/*'])
        if unexpected:
            fail('error: unexpected staged files:\n' + unexpected)
        libraries = [lib / (library + '.so.15.0.0') for library in LIBRARIES]
        tools.run(['strip', '--strip-unneeded', '--', *libraries])
        tools.run(['chmod', '0755', '--', *libraries])
        tools.run(['chmod', '0644', '--', *sorted(lib.glob('*.a')), *sorted(pc.glob('*.pc')), *sorted(inc.glob('*.h'))])
        env = {**tools.env, 'PKG_CONFIG_PATH': str(pc)}

        def gate(message):
            fail('GATE FAILED: ' + message)

        def query(*arguments):
            return tools.text(['pkg-config', *arguments], env=env)

        for library in LIBRARIES:
            names = re.findall(r'\(SONAME\).*\[(.*)\]', tools.text(['readelf', '-d', lib / (library + '.so.15.0.0')]))
            soname = '\n'.join(names)
            if soname != library + '.so.15':
                gate(f"{library} SONAME is '{soname}', want '{library}.so.15'")
        db_format = {'bdb': 'BerkeleyDB', 'kyotocabinet': 'KyotoCabinet', 'tkrzw': 'Tkrzw'}[backend]
        checks = [('libpinyin', 'database_format', db_format), ('libzhuyin', 'database_format', db_format),
                  ('libpinyin', 'libpinyin_binary_version', '15.0'), ('libzhuyin', 'libzhuyin_binary_version', '15.0')]
        for library, key, want in checks:
            got = query('--variable=' + key, library)
            if got != want:
                gate(f"{library} {key} is '{got}', want '{want}'")
        for library in LIBRARIES:
            if query('--modversion', library) != pcver:
                gate(f'{library} modversion is not {pcver}')
            if not re.search(r'(?<!\w)' + re.escape('-l' + library[3:]) + r'(?!\w)', query('--libs', library)):
                gate(f'{library} --libs does not contain -l{library[3:]}')
            if 'libpinyin-' + pcver not in query('--cflags', library):
                gate(f'{library} --cflags does not point into libpinyin-{pcver}')
        if not query('--variable=pkgdatadir', 'libpinyin'):
            gate('libpinyin pkgdatadir is empty (the silent-misconfiguration gap)')
        with tempfile.TemporaryDirectory() as smoke:
            for library, source in [('libpinyin', templates.SMOKE_PINYIN), ('libzhuyin', templates.SMOKE_ZHUYIN)]:
                name = library[3:]
                program = Path(smoke) / ('smoke_' + name)
                program.with_suffix('.c').write_text(source)
                # The source used shell word splitting, not shell evaluation.
                try:
                    tools.run(['cc', program.with_suffix('.c'), '-o', program, '-I' + str(inc), '-L' + str(lib),
                               *query('--cflags', '--libs', library).split()], env=env)
                except Finished:
                    gate(f'{library} smoke compile/link against the staged tree failed')
            env['LD_LIBRARY_PATH'] = str(lib) + (':' + tools.env['LD_LIBRARY_PATH'] if tools.env.get('LD_LIBRARY_PATH') else '')
            for library in LIBRARIES:
                program = Path(smoke) / ('smoke_' + library[3:])
                try:
                    check(backend, 'release-stage.sh', 'subject', program, env=env)
                    tools.run([program], env=env)
                except (RuntimeRefused, Finished) as error:
                    if isinstance(error, RuntimeRefused):
                        emit('REFUSED: ' + str(error), error=True)
                    gate(f'{library} smoke run failed')
        emit(f'== staged tree under {dest} ==')
        listing = tools.text(['find', dest, '-mindepth', '1', '-printf', '%y %P\n'])
        emit('\n'.join(sorted(listing.splitlines())))
        emit(f'== release-stage gates passed (backend: {backend}) ==')
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
    if not tools.which('pkg-config'):
        fail('error: pkg-config not found', 2)
    with directory(tools.here):
        backend = values['build_subject']
        if backend:
            tools.run(['rm', '-rf', '--', values['subject_root']])
            layout = [f'--prefix={values["prefix"]}', f'--destdir={values["subject_root"]}']
            layout += [f'--{key}={values[key]}' for key in ('libdir', 'includedir') if values[key]]
            for library in LIBRARIES:
                install([library, *layout, '--', '--profile', 'dev', '--locked', '--no-default-features',
                         '--features', backend + (',shipped' if library == 'libpinyin' else '')], tools)
        roots = []
        for key, label in [('pin_root', 'pin'), ('subject_root', 'subject')]:
            if not Path(values[key]).is_dir():
                fail(f'error: no {label} root', 2)
            roots.append(str(Path(values[key]).resolve()))
        prefix = values['prefix']
        libdir, includedir = values['libdir'] or prefix + '/lib', values['includedir'] or prefix + '/include'
        relocated, differences = '/relocated-by-check-pc-metadata', 0
        with tempfile.TemporaryDirectory() as work:
            def compare(label, texts):
                nonlocal differences
                paths = [Path(work) / 'pin', Path(work) / 'subject']
                for path, text in zip(paths, texts):
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


def function_order_command(argv, tools):
    if not argv or argv[0] not in ('inventory', 'resolve', 'policy'):
        fail('usage: tools/packaging/function-order.sh inventory ARCHIVE | resolve ARCHIVE POLICY SONAME | policy POLICY SONAME', 2)
    mode = argv[0]
    if len(argv) != {'inventory': 2, 'resolve': 4, 'policy': 3}[mode]:
        raise Finished(2)
    fields = {'inventory': dict(archive=argv[1]), 'resolve': dict(archive=argv[1], policy=argv[2] if len(argv) > 2 else '', soname=argv[3] if len(argv) > 3 else ''),
              'policy': dict(policy=argv[1], soname=argv[2] if len(argv) > 2 else '')}[mode]
    done = function_order.resolve(mode, **fields,
        run=lambda command: tools.capture(command, env={**tools.env, 'LC_ALL': 'C'}))
    write(done.stdout)
    write(done.stderr, error=True)
    return status(done.status)


def main(argv=None, *, tools=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    tools = tools or Tools()
    if not argv or argv[0] not in NAMES:
        emit('usage: python -m tools.dev packaging {' + ','.join(NAMES) + '} ...', error=True)
        return 2
    name, rest = argv[0], argv[1:]
    functions = {'install': install, 'relink-versioned': relink, 'check-pc-version': check_version,
                 'check-pc-metadata': metadata, 'function-order': function_order_command,
                 'release-stage': release_stage, 'release-deb': release_deb, 'release-rpm': release_rpm,
                 'release-arch': release_arch}
    def call():
        if name == 'pc-version':
            emit(version(tools.root))
            return 0
        return functions[name](rest, tools)
    return guarded(call)
