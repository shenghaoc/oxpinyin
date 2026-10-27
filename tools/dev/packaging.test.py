"""Packaging boundary fixtures; optional preserved-base comparisons, with no builds.

OXPINYIN_PACKAGING_BASE names the unmodified checkout for U4's old/new control.
Both entry points see the same private checkout, fakes, argv, paths and bytes.
The fixtures also run in CI without that control, asserting their expected
statuses and outputs. Fake compilers/builders use an explicit Tools adapter.
"""
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import packaging

BASE = Path(os.environ['OXPINYIN_PACKAGING_BASE']) if os.environ.get('OXPINYIN_PACKAGING_BASE') else None
DRIVER = '''import sys
from pathlib import Path
from tools.dev import packaging
class FixtureTools(packaging.Tools):
    def heavy(self, argv, **options):
        self.run(argv, **options)
sys.exit(packaging.main(sys.argv[2:], tools=FixtureTools(root=Path(sys.argv[1]))))
'''
FAKE_CARGO = r'''
import json, os, sys
from pathlib import Path
args=sys.argv[1:]
Path(os.environ['CALLS']).write_text(json.dumps(args))
if os.environ.get('FAIL_CARGO'):
    print('cargo failed', file=sys.stderr)
    sys.exit(int(os.environ['FAIL_CARGO']))
def value(flag, default=''):
    found=default
    for index, word in enumerate(args):
        if word == flag: found=args[index+1]
        elif word.startswith(flag+'='): found=word.split('=',1)[1]
    return found
library='libzhuyin' if 'zhuyin' in Path.cwd().name else 'libpinyin'
triple=Path(value('--target', os.environ.get('CARGO_BUILD_TARGET','fixture-target')).rstrip('/')).name
if triple.endswith('.json') and triple!='.json':triple=triple[:-5]
profile=value('--profile','release')
profile='debug' if profile=='dev' else profile
if '--debug' in args:profile='debug'
if '--release' in args:profile='release'
qualified=Path(value('--target-dir'))/triple/profile
qualified.mkdir(parents=True, exist_ok=True)
(qualified/(library+'.a')).write_bytes(b'static archive')
if not os.environ.get('MISSING_BAKED'):
    baked=qualified/(library+'.pc.in.baked')
    if os.environ.get('FALLBACK_BAKED'):
        baked=qualified/'build/pkg-hash/out'/baked.name
        baked.parent.mkdir(parents=True)
    text=(Path.cwd()/(library+'.pc.in')).read_text()
    text=text[text.index('prefix=@prefix@'):]
    for key,val in [('VERSION','2.11.92'),('LIBPINYIN_BINARY_VERSION','15.0'),('LIBZHUYIN_BINARY_VERSION','15.0'),('DATABASE_FORMAT',os.environ.get('LIBPINYIN_DATABASE_FORMAT','BerkeleyDB'))]:
        text=text.replace('@'+key+'@', val)
    baked.write_text(text)
if os.environ.get('AMBIGUOUS_BAKED'):
    for name in ['one','two']:
        other=qualified/'build'/name/'out'/(library+'.pc.in.baked')
        other.parent.mkdir(parents=True,exist_ok=True);other.write_text('stale')
lib=Path(value('--destdir')+value('--libdir'))
lib.mkdir(parents=True,exist_ok=True)
(lib/(library+'.so.15.0.0')).write_bytes(b'installed library')
(lib/(library+'.15.0.0.dylib')).write_bytes(b'installed dylib')
(lib/(library+'.a')).write_bytes(b'static archive')
for suffix,target in [('.so.15',library+'.so.15.0.0'),('.so',library+'.so.15')]:
    path=lib/(library+suffix)
    if path.is_symlink():path.unlink()
    path.symlink_to(target)
inc=Path(value('--destdir')+value('--includedir'))/'libpinyin-2.11.92'
inc.mkdir(parents=True,exist_ok=True)
for name in ['novel_types.h','pinyin_custom2.h', 'zhuyin_custom2.h' if library=='libzhuyin' else 'pinyin.h', 'zhuyin.h' if library=='libzhuyin' else 'pinyin.h']:
    shutil_source=Path.cwd()/name
    (inc/name).write_bytes(shutil_source.read_bytes())
'''


def tree(path):
    return {str(entry.relative_to(path)): ('link', os.readlink(entry)) if entry.is_symlink()
            else ('file', entry.read_bytes()) for entry in path.rglob('*') if not entry.is_dir()}


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='packaging-test-')
        self.addCleanup(self.temp.cleanup)
        self.work = Path(self.temp.name)
        self.root = self.work / 'checkout'
        for relative in ['crates/oxpinyin-capi', 'crates/oxpinyin-zhuyin-capi', 'tools/oracle', 'tools/lib', 'tools/tkrzw', 'tools/packaging']:
            (self.root / relative).mkdir(parents=True)
        for crate in packaging.LIBRARIES.values():
            for source in (ROOT / 'crates' / crate).iterdir():
                if source.is_file():
                    shutil.copyfile(source, self.root / 'crates' / crate / source.name)
        shutil.copyfile(ROOT / 'tools/oracle/oracle-pin.txt', self.root / 'tools/oracle/oracle-pin.txt')
        if BASE:
            # Only the preserved shell control sources the removed runtime shim.
            shutil.copy(BASE / 'tools/tkrzw/runtime.sh', self.root / 'tools/tkrzw/runtime.sh')
            for source in (BASE / 'tools/packaging').glob('*'):
                if source.is_file():
                    shutil.copy(source, self.root / 'tools/packaging' / source.name)
            shutil.copy(ROOT / 'tools/lib/bash-floor.sh', self.root / 'tools/lib/bash-floor.sh')
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.env = {**os.environ, 'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
                    'CALLS': str(self.work / 'calls'), 'CARGO_BUILD_TARGET': 'fixture-target'}
        self.env.pop('CARGO_TARGET_DIR', None)
        self.fake('cargo', FAKE_CARGO)
        # A host with no relink lane: the Linux relink has RelinkTests and the
        # Mach-O relink MachoRelinkTests, both in tools/packaging/function-order.test.py.
        self.fake('uname', "print('fixture-host')")
        self.fake('rustc', "print('host: fixture-target')")
        self.fake('dpkg', "print('fixture-architecture')")
        self.fake('nm','pass')
        self.fake('readelf','pass')
        self.fake('chmod', r"""import os,sys
from pathlib import Path
mode=sys.argv[1]
for word in sys.argv[2:]:
    if word=='--':continue
    path=Path(word)
    os.chmod(path,int(mode,8) if mode.isdecimal() else path.stat().st_mode|0o222)
""")

        # Distro packaging fixtures provide their GNU host interfaces on macOS
        # too; they never require a Debian installation or system GLib data.
        pcdir = self.work / 'system-pc'
        pcdir.mkdir()
        for module in ('glib-2.0', 'gobject-2.0'):
            (pcdir / (module + '.pc')).write_text('Name: '+module+'\nDescription: fixture\nVersion: 2.88.0\nLibs: -l'+module+'\nCflags:\n')
        self.env['PKG_CONFIG_LIBDIR'] = str(pcdir)
        self.fake('md5sum', "import hashlib,sys; from pathlib import Path; "
                  "[(print(hashlib.md5(Path(name).read_bytes()).hexdigest()+'  '+name,flush=True)) for name in sys.argv[1:]]")
        self.fake('sort', r"""import sys
from pathlib import Path
files=[word for word in sys.argv[1:] if not word.startswith('-')]
data=b''.join(Path(file).read_bytes() for file in files) if files else sys.stdin.buffer.read()
flags=''.join(word for word in sys.argv[1:] if word.startswith('-'))
separator=b'\0' if 'z' in flags else b'\n'
lines=data.split(separator)
if lines and lines[-1]==b'':lines.pop()
if 'u' in flags:lines=list(set(lines))
key=(lambda value:tuple(int(part) for part in value.split(b'.'))) if 'V' in flags else None
if lines:sys.stdout.buffer.write(separator.join(sorted(lines,key=key))+separator)
""")

        shutil.copy(ROOT / 'tools/oracle/build-oracle.sh', self.root / 'tools/oracle/build-oracle.sh')
        (self.root / 'tools/install-python.sh').write_text('#!/bin/sh\nexit 0\n')
        (self.root / 'tools/install-python.sh').chmod(0o755)
        self.fake('uv', 'import sys; from pathlib import Path; sys.path.insert(0, ' + repr(str(ROOT)) + '); from tools.dev.cell import main; sys.exit(main(["--shell"], root=Path.cwd()))')


    def fake(self, name, source):
        path = self.bin / name
        path.write_text('#!' + sys.executable + '\n' + source)
        path.chmod(0o755)

    def invoke(self, name, args=(), *, old=False, env=None):
        command = (['bash', str(self.root / 'tools/packaging' / (name + '.sh'))] if old else
                   [sys.executable, '-X', 'lazy_imports=normal', '-c', DRIVER, str(self.root), name])
        return subprocess.run([*command, *map(str, args)], env={**self.env, **(env or {})},
                              cwd=ROOT, capture_output=True, timeout=20)

    def normalize(self, result):
        def normal(data):
            data = data.replace(os.fsencode(str(self.root) + '/'), b'')
            return re.sub(rb'(?:tmp\.[A-Za-z0-9]+|tmp[A-Za-z0-9_]+)', b'TEMP', data)
        return result.returncode, normal(result.stdout), normal(result.stderr)

    def compare(self, name, args=(), *, env=None, reset=None, files=None, diagnostics=True, artifacts=()):
        result = self.invoke(name, args, env=env)
        contents = tree(files) if files else None
        calls = self.work / 'calls'
        arguments = calls.read_bytes() if calls.exists() else None
        produced = [path.read_bytes() for path in artifacts]
        if BASE:
            if reset:
                reset()
            control = self.invoke(name, args, old=True, env=env)
            self.assertEqual(result.returncode, control.returncode)
            if arguments is not None:
                self.assertEqual(arguments, calls.read_bytes())
            if diagnostics:
                self.assertEqual(self.normalize(result), self.normalize(control))
            if files:
                self.assertEqual(contents, tree(files))
            self.assertEqual(produced, [path.read_bytes() for path in artifacts])
        return result

    def install(self, library='libpinyin', extra=(), env=None):
        stage = self.work / 'stage'
        def reset():
            shutil.rmtree(stage, ignore_errors=True)
            shutil.rmtree(self.root / 'target', ignore_errors=True)
        reset()
        result = self.compare('install', [library, '--prefix=/usr', '--destdir=' + str(stage), *extra],
                              env=env, reset=reset, files=stage)
        return result, stage

    def test_metadata_usage_equals_complete_preserved_shell_header(self):
        from tools.dev.packaging_templates import METADATA_USAGE
        # Complete usage header from e79802e8:tools/packaging/check-pc-metadata.sh.
        # Kept independently of the production template and available in shallow CI.
        preserved = r'''Usage: tools/packaging/check-pc-metadata.sh --pin-root=DIR --subject-root=DIR
           --prefix=DIR [--libdir=DIR] [--includedir=DIR]
           [--build-subject=FEATURES] [--modules="libpinyin libzhuyin"]
  --build-subject  run install.sh for both libraries into --subject-root
                   first (emptying it), with `--profile dev --locked
                   --no-default-features --features FEATURES` (libpinyin
                   also gets `shipped`); FEATURES is the backend
                   (bdb|kyotocabinet|tkrzw).
  --libdir / --includedir  the values BOTH sides were installed with;
                   omitted means autoconf's defaults (<prefix>/lib,
                   <prefix>/include), the tree layout either side produces
                   without the flag.

Exits 0 when every item matches; 1 on any difference (each one printed as a
unified diff); 2 on a usage error or a missing input.
'''
        self.assertEqual(METADATA_USAGE, preserved)
        if BASE:
            source = (BASE / 'tools/packaging/check-pc-metadata.sh').read_text()
            header = source[source.index('# Usage:'):source.index('\n\n# shellcheck')]
            self.assertEqual(preserved, '\n'.join(
                line[2:] if line.startswith('# ') else '' for line in header.splitlines()) + '\n')

    def test_pin_version_and_static_declarations(self):
        for name in ('pc-version', 'check-pc-version'):
            result = self.compare(name)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(b'2.11.92', result.stdout)

    def test_missing_and_malformed_pin_status(self):
        for text in ('libpinyin_tag=broken\n', 'no tag\n', 'libpinyin_tag=2.11.92\r\n'):
            (self.root / 'tools/oracle/oracle-pin.txt').write_text(text)
            result = self.compare('pc-version')
            self.assertEqual(result.returncode, 1)
            self.assertIn(b'no well-formed', result.stderr)

    def test_usage_and_option_statuses(self):
        for name, arguments in [('install', []), ('install', ['both', '--prefix=/usr']),
                                 ('install', ['libpinyin', '--prefix']), ('relink-versioned', []),
                                 ('relink-versioned', ['--unknown']), ('release-stage', []),
                                 ('release-stage', ['bdb','--prefix=/usr','--libdir=/usr/lib','--dest=/']),
                                 ('release-arch', ['tkrzw','1','stage','out','--data-version=2']),
                                 ('function-order', []), ('function-order', ['resolve']),
                                 ('check-pc-metadata', []), ('check-pc-metadata', ['--unknown'])]:
            with self.subTest(name=name, arguments=arguments):
                # The old usage printer stopped at its first "Exits" line.
                # The complete preserved header is now restored and checked above.
                result = self.compare(name, arguments, diagnostics=name != 'check-pc-metadata')
                self.assertEqual(result.returncode, 2)
                if name == 'check-pc-metadata':
                    from tools.dev.packaging_templates import METADATA_USAGE
                    self.assertTrue(result.stderr.endswith(METADATA_USAGE.encode()))

    def test_missing_pin_and_command_execution_statuses(self):
        (self.root / 'tools/oracle/oracle-pin.txt').unlink()
        self.assertEqual(self.compare('pc-version').returncode, 2)
        self.env['PATH'] = str(self.bin) + ':/usr/bin:/bin'
        (self.bin / 'cargo').chmod(0o644)
        args = ['libpinyin','--prefix=/usr','--destdir='+str(self.work/'stage')]
        self.assertEqual(self.compare('install',args,diagnostics=False).returncode,126)
        (self.bin / 'cargo').unlink()
        args = ['libpinyin','--prefix=/usr','--destdir='+str(self.work/'stage')]
        self.assertEqual(self.compare('install',args,diagnostics=False).returncode,127)

    def test_package_required_arguments_keep_status(self):
        for name in ('release-deb', 'release-rpm', 'release-arch'):
            result = self.compare(name, diagnostics=False)
            self.assertEqual(result.returncode, 1)
            self.assertIn(b'backend required', result.stderr)

    def test_each_facade_installs_separately_and_metadata_is_relocatable(self):
        for library in packaging.LIBRARIES:
            with self.subTest(library=library):
                result, stage = self.install(library)
                self.assertEqual(result.returncode, 0, result.stderr)
                files = tree(stage)
                self.assertIn('usr/lib/' + library + '.a', files)
                other = 'libzhuyin' if library == 'libpinyin' else 'libpinyin'
                self.assertNotIn('usr/lib/' + other + '.a', files)
                pc = (stage / 'usr/lib/pkgconfig' / (library + '.pc')).read_bytes()
                self.assertIn(b'exec_prefix=${prefix}\nlibdir=${exec_prefix}/lib\nincludedir=${prefix}/include\n', pc)
                shutil.rmtree(stage)

    def test_macos_install_runs_the_mach_o_relink_lane(self):
        # The Darwin host selects the facade's checked-in policy and relinks
        # the installed dylib (MachoRelinkTests covers the relink itself); an
        # explicit opt-out keeps cargo-c's object. No preserved-base control:
        # the shell entry points had no Mach-O lane. Every tool the lane
        # consults before the policy is a fake here, so the outcome is the
        # same on a Linux host, which has no otool.
        self.fake('uname', "print('Darwin')")
        for tool in ('cc', 'nm', 'otool'):
            self.fake(tool, 'pass')
        self.env.pop('CC', None)
        stage = self.work / 'stage'
        for library in packaging.LIBRARIES:
            with self.subTest(library=library):
                shutil.rmtree(stage, ignore_errors=True)
                result = self.invoke('install', [library, '--prefix=/usr', '--destdir=' + str(stage)],
                                     env={'CARGO_BUILD_TARGET': 'aarch64-apple-darwin'})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn(b'policy not found: ' + os.fsencode(str(self.root / 'tools/packaging/function-order/bdb' / (library + '.order'))),
                              result.stderr)
                self.assertIn(b'(checked-in)', result.stderr)
                self.assertEqual((stage / 'usr/lib' / (library + '.15.0.0.dylib')).read_bytes(), b'installed dylib')
                shutil.rmtree(stage, ignore_errors=True)
                result = self.invoke('install', [library, '--prefix=/usr', '--destdir=' + str(stage)],
                                     env={'CARGO_BUILD_TARGET': 'aarch64-apple-darwin', 'OXPINYIN_FUNCTION_ORDER_POLICY': ''})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(b'disabled by explicit opt-out; keeping the original Mach-O layout', result.stderr)
                self.assertEqual((stage / 'usr/lib' / (library + '.15.0.0.dylib')).read_bytes(), b'installed dylib')

    def macos_fakes(self):
        """Fakes for every tool either relink consults; cc and rustc record their arguments.

        nm answers each of the relinks' questions from the version script
        FIXTURE_VER names: the Mach-O views of one text symbol, the exported
        symbols, and the versioned ELF exports.
        """
        self.fake('uname', "print('Darwin')")
        self.fake('rustc', r"""import json,os,sys
from pathlib import Path
args=sys.argv[1:]
Path(os.environ['RUSTC_CALLS']).write_text(json.dumps(args))
print('host: fixture-target' if '-vV' in args else 'MACOSX_DEPLOYMENT_TARGET=11.0')
""")
        self.fake('cc', r"""import json,os,sys
from pathlib import Path
args=sys.argv[1:]
Path(os.environ['CC_CALLS']).write_text(json.dumps(args))
if '-o' in args: Path(args[args.index('-o')+1]).write_bytes(b'synthetic linked bytes')
""")
        self.fake('nm', r"""import os,re,sys
from pathlib import Path
args=sys.argv[1:]
names=re.findall(r'\b(?:pinyin|zhuyin)_\w+(?=\s*;)', Path(os.environ['FIXTURE_VER']).read_text())
if '--format=darwin' in args:
    print('0000000000000100 (__TEXT,__text) external _'+names[0])
elif '-g' in args:
    for name in names: print('0000000000000100 T _'+name)
elif '-D' in args:
    print('00000000 A LIBPINYIN')
    for name in names: print('00000001 T '+name+'@@LIBPINYIN')
else:
    print('0000000000000100 T _'+names[0])
""")
        self.fake('otool', "print('Section\\n  sectname __text\\n   segname __TEXT\\n      addr 0x0000000000000100\\n      size 0x10')")
        self.fake('readelf', "print(' 0x000000000000000e (SONAME) Library soname: [libpinyin.so.15]')")
        self.env.update(CC_CALLS=str(self.work / 'cc-calls'), RUSTC_CALLS=str(self.work / 'rustc-calls'))
        for name in ('CC', 'OXPINYIN_BDB_LIB_DIR', 'OXPINYIN_FUNCTION_ORDER_POLICY', 'OXPINYIN_RELINK_FEATURES'):
            self.env.pop(name, None)

    def relink_inputs(self, dest):
        archive, ver, policy = (self.work / name for name in ('library.a', 'library.ver', 'policy.order'))
        archive.write_bytes(b'synthetic archive')
        ver.write_text('LIBPINYIN {\n global:\n pinyin_init;\n local:\n *;\n};\n')
        policy.write_text('oxpinyin-function-order-v1\tlibpinyin.so.15\ninit\tpinyin_init\n')
        dest.write_bytes(b'original installed bytes')
        for name in ('cc-calls', 'rustc-calls'):
            (self.work / name).unlink(missing_ok=True)
        return archive, ver, policy

    def recorded(self, name):
        path = self.work / name
        return json.loads(path.read_text()) if path.exists() else None

    def macos_relink(self, arguments):
        dest = self.work / 'libpinyin.15.0.0.dylib'
        archive, ver, policy = self.relink_inputs(dest)
        result = self.invoke('relink-macho', ['--staticlib', archive, '--ver', ver, '--soname', 'libpinyin.so.15',
                                              '--dest', dest, '--install-name', '/opt/stage/lib/libpinyin.15.dylib',
                                              '--function-order', policy, *arguments], env={'FIXTURE_VER': str(ver)})
        return result, self.recorded('cc-calls'), self.recorded('rustc-calls'), dest.read_bytes()

    def test_darwin_relink_links_the_kyoto_cabinet_shim_against_libcxx(self):
        self.macos_fakes()
        result, cc, _, data = self.macos_relink(['--features', 'kyotocabinet'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(data, b'synthetic linked bytes')
        self.assertIn('-lkyotocabinet', cc)
        self.assertEqual(cc[cc.index('-lkyotocabinet') + 1], '-lc++')
        self.assertNotIn('-lstdc++', cc)
        # The Linux relink keeps libstdc++, the runtime its shim is built against.
        self.fake('uname', "print('Linux')")
        dest = self.work / 'libpinyin.so.15.0.0'
        archive, ver, _ = self.relink_inputs(dest)
        result = self.invoke('relink-versioned', ['--staticlib', archive, '--ver', ver, '--soname', 'libpinyin.so.15',
                                                  '--dest', dest, '--features', 'kyotocabinet', '--function-order', ''],
                             env={'FIXTURE_VER': str(ver)})
        self.assertEqual(result.returncode, 0, result.stderr)
        cc = self.recorded('cc-calls')
        self.assertEqual(cc[cc.index('-lkyotocabinet') + 1], '-lstdc++')
        self.assertNotIn('-lc++', cc)
        self.assertEqual(dest.read_bytes(), b'synthetic linked bytes')

    def test_darwin_relink_target_selects_the_architecture_and_deployment_target(self):
        self.macos_fakes()
        for triple, arch in (('x86_64-apple-darwin', 'x86_64'), ('aarch64-apple-darwin', 'arm64')):
            with self.subTest(triple=triple):
                result, cc, rustc, data = self.macos_relink(['--target', triple])
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(data, b'synthetic linked bytes')
                self.assertEqual(cc[:3], ['-dynamiclib', '-arch', arch])
                self.assertEqual(rustc, ['--print', 'deployment-target', '--target', triple])
                self.assertIn('-mmacosx-version-min=11.0', cc)

    def test_darwin_install_passes_the_build_target_to_the_relink(self):
        self.macos_fakes()
        policy = self.root / 'tools/packaging/function-order/bdb/libpinyin.order'
        policy.parent.mkdir(parents=True)
        policy.write_text('oxpinyin-function-order-v1\tlibpinyin.so.15\ninit\tpinyin_init\n')
        stage = self.work / 'stage'
        result = self.invoke('install', ['libpinyin', '--prefix=/usr', '--destdir=' + str(stage)],
                             env={'CARGO_BUILD_TARGET': 'x86_64-apple-darwin',
                                  'FIXTURE_VER': str(self.root / 'crates/oxpinyin-capi/libpinyin.ver')})
        self.assertEqual(result.returncode, 0, result.stderr)
        cc, rustc = self.recorded('cc-calls'), self.recorded('rustc-calls')
        self.assertEqual(cc[:3], ['-dynamiclib', '-arch', 'x86_64'])
        self.assertEqual(rustc, ['--print', 'deployment-target', '--target', 'x86_64-apple-darwin'])
        self.assertIn('-Wl,-u,_pinyin_init', cc)
        self.assertIn(os.fsdecode(str(self.root / 'target/x86_64-apple-darwin/release/libpinyin.a')), cc)
        self.assertEqual((stage / 'usr/lib/libpinyin.15.0.0.dylib').read_bytes(), b'synthetic linked bytes')
        self.assertIn(b'relinked ', result.stdout)

    def test_darwin_relink_refuses_a_target_that_is_not_apple_darwin(self):
        self.macos_fakes()
        for triple in ('x86_64-unknown-linux-gnu', 'fixture-target', 'i686-apple-darwin'):
            with self.subTest(triple=triple):
                result, cc, rustc, data = self.macos_relink(['--target', triple])
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIsNone(cc)
                self.assertIsNone(rustc)
                self.assertEqual(data, b'original installed bytes')
                self.assertIn(b'error: relink-macho', result.stderr)
                self.assertIn(triple.encode(), result.stderr)

    def test_darwin_relink_without_target_keeps_the_host_command_lines(self):
        self.macos_fakes()
        result, cc, rustc, data = self.macos_relink([])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(rustc, ['--print', 'deployment-target'])
        self.assertNotIn('-arch', cc)
        archive = cc.index(str(self.work / 'library.a'))
        scratch = os.path.dirname(cc[4].removeprefix('-Wl,-order_file,'))
        self.assertEqual(cc[:archive], [
            '-dynamiclib', '-mmacosx-version-min=11.0', '-o', cc[3],
            '-Wl,-order_file,' + scratch + '/order.txt', '-Wl,-S', '-Wl,-dead_strip',
            '-Wl,-exported_symbols_list,' + scratch + '/exports.txt',
            '-Wl,-install_name,/opt/stage/lib/libpinyin.15.dylib', '-Wl,-compatibility_version,15',
            '-Wl,-current_version,15.0.0', '-Wl,-headerpad_max_install_names', '-Wl,-u,_pinyin_init'])
        self.assertTrue(cc[3].startswith(str(self.work / 'libpinyin.15.0.0.relink.')))
        self.assertEqual(sorted(cc[archive + 1:archive + 3]), ['-lglib-2.0', '-lgobject-2.0'])
        self.assertEqual(cc[archive + 3:], ['-ldb', '-liconv'])

    def test_explicit_layout_and_special_replacement_bytes(self):
        result, stage = self.install(extra=['--libdir=/usr/lib & hash#dir', '--includedir=/usr/inc\\lude#&'])
        self.assertEqual(result.returncode, 0, result.stderr)
        data = (stage / 'usr/lib & hash#dir/pkgconfig/libpinyin.pc').read_bytes()
        self.assertIn(b'libdir=/usr/lib & hash#dir\n', data)
        self.assertIn(b'includedir=/usr/inc\\lude#&\n', data)
        self.assertNotIn(os.fsencode(stage), data)

    def test_forwarding_custom_target_profile_and_empty_words(self):
        target = self.work / 'custom target'
        actual = self.work / 'physical target'
        actual.mkdir()
        target.symlink_to(actual, target_is_directory=True)
        result, _ = self.install(extra=['--', '--target-dir', str(target), '--target=specs/fake.json',
                                          '--profile', 'dev', '--locked', '--features=a', '--features', 'b,c', ''])
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads((self.work / 'calls').read_text())
        self.assertEqual(sum(word.startswith('--target-dir=') for word in args), 1)
        self.assertIn('--target-dir=' + str(target), args)
        self.assertEqual(args[-1], '')
        self.assertTrue((target / 'fake/debug/libpinyin.pc.in.baked').is_file())

    def test_custom_target_whose_entire_name_is_the_json_suffix(self):
        result, _ = self.install(extra=['--', '--target=specs/.json', '--debug'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / 'target/.json/debug/libpinyin.pc.in.baked').is_file())

    def test_target_environment_is_relative_to_packaging_directory(self):
        result, _ = self.install(env={'CARGO_TARGET_DIR': 'relative target'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / 'tools/packaging/relative target/fixture-target/release/libpinyin.pc.in.baked').is_file())

    def test_baked_fallback_and_ambiguity(self):
        result, _ = self.install(env={'FALLBACK_BAKED': '1'})
        self.assertEqual(result.returncode, 0, result.stderr)
        shutil.rmtree(self.root / 'target')
        for env, message in [({'MISSING_BAKED':'1'}, b'not found for this build'),
                             ({'MISSING_BAKED':'1', 'AMBIGUOUS_BAKED':'1'}, b'multiple')]:
            result, _ = self.install(env=env)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn(message, result.stderr)
            shutil.rmtree(self.root / 'target')

    def test_cargo_failure_status_and_no_complete_pc(self):
        result, stage = self.install(env={'FAIL_CARGO':'37'})
        self.assertEqual(result.returncode, 37)
        self.assertFalse((stage / 'usr/lib/pkgconfig/libpinyin.pc').exists())

    def test_companion_header_refusal_precedes_cargo(self):
        (self.root / 'crates/oxpinyin-zhuyin-capi/novel_types.h').write_bytes(b'changed')
        result, _ = self.install()
        self.assertEqual(result.returncode, 1)
        self.assertIn(b'companion header', result.stderr)
        self.assertFalse((self.work / 'calls').exists())

    def test_arch_validation_and_missing_package_inputs(self):
        for name, args, expected in [
            ('release-arch',['bdb','1','stage','out'],2),
            ('release-arch',['tkrzw','1','stage','out','--data=absent'],1),
            ('release-arch',['tkrzw','1','stage','out','--bad'],2),
            ('release-rpm',['tkrzw','1','stage','out'],1),
            ('release-deb',['bdb','1','stage','/usr/lib','out'],1)]:
            with self.subTest(name=name, args=args):
                self.assertEqual(self.compare(name,args).returncode, expected)


    def test_installed_metadata_matches_and_reports_a_real_difference(self):
        result, stage = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        pin = self.work / 'pin'
        shutil.copytree(stage, pin)
        arguments = ['--pin-root=' + str(pin), '--subject-root=' + str(stage), '--prefix=/usr', '--modules=libpinyin']
        result = self.compare('check-pc-metadata', arguments)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'OK: packaging metadata matches', result.stdout)
        path = stage / 'usr/lib/pkgconfig/libpinyin.pc'
        path.write_bytes(path.read_bytes().replace(b'BerkeleyDB', b'Tkrzw'))
        result = self.compare('check-pc-metadata', arguments)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn(b'FAIL:', result.stderr)

    def staged(self):
        stage = self.work / 'package-stage'
        lib = stage / 'usr/lib'
        (lib / 'pkgconfig').mkdir(parents=True)
        inc = stage / 'usr/include/libpinyin-2.11.92'
        inc.mkdir(parents=True)
        for library in packaging.LIBRARIES:
            for suffix in ('.a', '.so.15.0.0'):
                (lib / (library + suffix)).write_bytes(b'payload')
            (lib / (library + '.so.15')).symlink_to(library + '.so.15.0.0')
            (lib / (library + '.so')).symlink_to(library + '.so.15')
            (lib / 'pkgconfig' / (library + '.pc')).write_bytes(b'metadata')
        (inc / 'header.h').write_bytes(b'header')
        return stage

    def test_deb_payload_metadata_and_builder_failures(self):
        stage, out = self.staged(), self.work / 'packages'
        self.fake('objdump', "import sys; print(' NEEDED libc.so.6\\n NEEDED libglib.so.0' if sys.argv[1]=='-p' else 'GLIBC_2.17\\nGLIBC_2.9')")
        self.fake('ldconfig', "print('libc.so.6 => /lib/libc.so.6\\nlibglib.so.0 => /lib/libglib.so.0')")
        self.fake('dpkg', "import sys; print('amd64' if sys.argv[1]=='--print-architecture' else ('libc6: /lib/libc.so.6' if sys.argv[-1].endswith('libc.so.6') else 'libglib2.0-0t64: /lib/libglib.so.0'))")
        self.fake('dpkg-deb', """import os, shutil, sys
from pathlib import Path
if os.environ.get('FAIL_PACKAGE'): sys.exit(39)
shutil.copytree(sys.argv[-2], Path(os.environ['PAYLOAD'])/Path(sys.argv[-2]).name, dirs_exist_ok=True, symlinks=True)
package=Path(sys.argv[-1])
package.write_bytes(b'deb package')
os.utime(package,(1,1))
""")
        payload = self.work / 'payload'
        def reset():
            shutil.rmtree(out, ignore_errors=True)
            shutil.rmtree(payload, ignore_errors=True)
        result = self.compare('release-deb', ['bdb','0.1.0',stage,'/usr/lib',out],
                              env={'PAYLOAD':str(payload)}, reset=reset, files=payload)
        self.assertEqual(result.returncode, 0, result.stderr)
        control = (payload / 'runtime/DEBIAN/control').read_text()
        self.assertIn('Depends: libc6 (>= 2.17), libglib2.0-0t64', control)
        self.assertIn('Provides: libpinyin15 (= 2.11.92), libzhuyin15 (= 2.11.92)', control)
        self.assertIn('Conflicts: libpinyin15, libzhuyin15', control)
        result = self.compare('release-deb', ['bdb','0.1.0',stage,'/usr/lib',out], env={'FAIL_PACKAGE':'1'})
        self.assertEqual(result.returncode, 39)

    def test_rpm_spec_payload_and_no_output_refusal(self):
        stage, out = self.staged(), self.work / 'packages'
        payload = self.work / 'rpm-spec'
        self.fake('rpmbuild', """import os, shutil, sys
from pathlib import Path
root=Path(sys.argv[2].removeprefix('_topdir '))
shutil.copyfile(sys.argv[-1], os.environ['PAYLOAD'])
if not os.environ.get('NO_RPM'):
    (root/'RPMS/x86_64').mkdir()
    package=root/'RPMS/x86_64/package.rpm'
    package.write_bytes(b'rpm package')
    os.utime(package,(1,1))
""")
        self.fake('rpm', 'print("/usr/lib/libpinyin.so.15")')
        result = self.compare('release-rpm', ['tkrzw','0.1.0',stage,out], env={'PAYLOAD':str(payload)}, artifacts=(payload,))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Provides:       libpinyin = 2.11.92 libzhuyin = 2.11.92', payload.read_text())
        self.assertIn('Obsoletes:      libpinyin libzhuyin', payload.read_text())
        result = self.compare('release-rpm', ['tkrzw','0.1.0',stage,out], env={'PAYLOAD':str(payload),'NO_RPM':'1'})
        self.assertEqual(result.returncode, 1)
        self.assertIn(b'produced no RPMs', result.stderr)

    def test_arch_recipe_bundles_data_and_preserves_identity(self):
        stage, out = self.staged(), self.work / 'packages'
        data, payload = self.work / 'data', self.work / 'arch-recipe'
        data.mkdir()
        (data / 'table.conf').write_bytes(b'data')
        for name in ('id', 'chown', 'useradd'):
            self.fake(name, 'pass')
        self.fake('uname', "print('x86_64')")
        self.fake('su', """import os, re, shutil, sys
from pathlib import Path
command=sys.argv[-1]
work=Path(re.search("cd '([^']+)'",command)[1])
out=Path(re.search("PKGDEST='([^']+)'",command)[1])
shutil.copyfile(work/'PKGBUILD', os.environ['PAYLOAD'])
shutil.copyfile(work/'stage-usr/lib/libpinyin/data/table.conf', os.environ['PAYLOAD']+'.data')
package=out/'oxpinyin-libpinyin-kyotocabinet-0.1.0-1-x86_64.pkg.tar.zst'
package.write_bytes(b'arch package')
os.utime(package,(1,1))
""")
        result = self.compare('release-arch', ['kyotocabinet','0.1.0',stage,out,'--data='+str(data),'--data-version=2.10.3-2'], env={'PAYLOAD':str(payload)}, artifacts=(payload,))
        self.assertEqual(result.returncode, 0, result.stderr)
        text = payload.read_text()
        self.assertIn('model data from Arch libpinyin 2.10.3-2', text)
        self.assertIn('libpinyin.so=15-64 libzhuyin.so=15-64', text)
        self.assertEqual(Path(str(payload)+'.data').read_bytes(), b'data')


    def test_relink_cancellation_status_original_file_and_cleanup(self):
        self.fake('uname', "print('Linux')")
        archive, ver, dest = [self.work / name for name in ('library.a','library.ver','libpinyin.so.15.0.0')]
        archive.write_bytes(b'archive')
        ver.write_text('LIBPINYIN {\n global:\n pinyin_init;\n local:\n *;\n};\n')
        ready = self.work / 'ready'
        self.fake('cc', "import os,time; from pathlib import Path; Path(os.environ['READY']).write_text('ready'); time.sleep(.8)")
        arguments = ['--staticlib',str(archive),'--ver',str(ver),'--soname','libpinyin.so.15','--dest',str(dest),'--function-order','']
        for sig in (signal.SIGINT, signal.SIGTERM):
            outcomes = []
            for old in ([False, True] if BASE else [False]):
                dest.write_bytes(b'original')
                ready.unlink(missing_ok=True)
                command = (['bash',str(self.root/'tools/packaging/relink-versioned.sh')] if old else
                           [sys.executable,'-X','lazy_imports=normal','-c',DRIVER,str(self.root),'relink-versioned'])
                with subprocess.Popen([*command,*arguments],cwd=ROOT,env={**self.env,'READY':str(ready)},
                                      stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True) as process:
                    deadline = time.monotonic()+5
                    while not ready.exists() and process.poll() is None and time.monotonic()<deadline:
                        time.sleep(.01)
                    self.assertTrue(ready.exists(), 'compiler fixture did not start')
                    process.send_signal(sig)
                    stdout, stderr = process.communicate(timeout=10)
                    self.assertEqual(process.returncode,128+sig)
                    outcomes.append((process.returncode,stdout,stderr))
                self.assertEqual(dest.read_bytes(),b'original')
                self.assertEqual(list(self.work.glob('*.relink.*')),[])
            if BASE:
                self.assertEqual(outcomes[0],outcomes[1])

    def stage_fakes(self):
        self.env['CHECKOUT'] = str(self.root)
        self.env['OXPINYIN_FUNCTION_ORDER_POLICY'] = ''
        self.fake('uname', "print('Linux')")
        self.fake('strip','pass')
        self.fake('ldd','pass')
        self.fake('find', r"""import fnmatch,sys
from pathlib import Path
root=Path(sys.argv[1])
if '-printf' in sys.argv:
    for path in root.rglob('*'):
        print(('l' if path.is_symlink() else 'd' if path.is_dir() else 'f')+' '+str(path.relative_to(root)))
else:
    patterns=[sys.argv[index+1] for index,word in enumerate(sys.argv) if word=='-path']
    for path in root.rglob('*'):
        if path.is_file() and not path.is_symlink() and not any(fnmatch.fnmatch(str(path),pattern) for pattern in patterns):print(path)
""")

        self.fake('cargo-cinstall','pass')
        self.fake('cc', r"""import sys
from pathlib import Path
args=sys.argv[1:]
out=Path(args[args.index('-o')+1])
if '-shared' in args:
    library=next(word for word in args if word.startswith('-Wl,-soname,')).split(',')[-1].split('.so')[0]
    out.write_text(library)
else:
    name='zhuyin' if 'zhuyin' in args[0] else 'pinyin'
    out.write_text('#!/bin/sh\necho '+name+'=fixture\n')
    out.chmod(0o755)
""")
        self.fake('nm', r"""import os,sys
from pathlib import Path
library=Path(sys.argv[-1]).read_text()
crate='oxpinyin-zhuyin-capi' if library=='libzhuyin' else 'oxpinyin-capi'
text=(Path(os.environ['CHECKOUT'])/'crates'/crate/(library+'.ver')).read_text()
node='LIBZHUYIN' if library=='libzhuyin' else 'LIBPINYIN'
print('00000000 A '+node)
for line in text.split('global:')[1].split('local:')[0].splitlines():
    name=line.strip().strip(';')
    if name:print('00000001 T '+name+'@@'+node)
""")
        self.fake('readelf', "import sys; from pathlib import Path; print(' (SONAME) Library soname: ['+Path(sys.argv[-1]).read_text()+'.so.15]')")

    def test_release_stage_complete_tree_and_gate_failure(self):
        self.stage_fakes()
        stage = self.work / 'stage'
        arguments = ['bdb','--prefix=/usr','--libdir=/usr/lib','--dest='+str(stage)]
        result = self.compare('release-stage',arguments,files=stage)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn(b'release-stage gates passed (backend: bdb)',result.stdout)
        self.assertIn(b'pinyin=fixture',result.stdout)
        self.assertIn(b'zhuyin=fixture',result.stdout)
        result = self.compare('release-stage',arguments,env={'LIBPINYIN_DATABASE_FORMAT':'Tkrzw'},files=stage)
        self.assertEqual(result.returncode,1,result.stderr)
        self.assertIn(b'GATE FAILED: libpinyin database_format',result.stderr)


if __name__ == '__main__':
    unittest.main()
