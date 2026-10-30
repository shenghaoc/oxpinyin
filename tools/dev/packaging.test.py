"""Packaging boundary fixtures with no builds.

Assert statuses, payloads and outputs using explicit fake Tools adapters.
"""
import json
import os
from pathlib import Path
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
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.env = {**os.environ, 'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
                    'CALLS': str(self.work / 'calls'), 'CARGO_BUILD_TARGET': 'fixture-target'}
        self.env.pop('CARGO_TARGET_DIR', None)
        self.fake('cargo', FAKE_CARGO)
        # The default fixture host needs no final relink.
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

    def invoke(self, name, args=(), *, env=None):
        command = [sys.executable, '-X', 'lazy_imports=normal', '-c', DRIVER, str(self.root), name]
        return subprocess.run([*command, *map(str, args)], env={**self.env, **(env or {})},
                              cwd=ROOT, capture_output=True, timeout=20)

    def install(self, library='libpinyin', extra=(), env=None):
        stage = self.work / 'stage'
        shutil.rmtree(stage, ignore_errors=True)
        shutil.rmtree(self.root / 'target', ignore_errors=True)
        result = self.invoke('install', [library, '--prefix=/usr', '--destdir=' + str(stage), *extra],
                              env=env)
        return result, stage

    def test_pin_version_and_static_declarations(self):
        for name in ('pc-version', 'check-pc-version'):
            result = self.invoke(name)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(b'2.11.92', result.stdout)

    def test_missing_and_malformed_pin_status(self):
        for text in ('libpinyin_tag=broken\n', 'no tag\n', 'libpinyin_tag=2.11.92\r\n'):
            (self.root / 'tools/oracle/oracle-pin.txt').write_text(text)
            result = self.invoke('pc-version')
            self.assertEqual(result.returncode, 1)
            self.assertIn(b'no well-formed', result.stderr)

    def test_usage_and_option_statuses(self):
        for name, arguments in [('install', []), ('install', ['both', '--prefix=/usr']),
                                 ('install', ['libpinyin', '--prefix']), ('relink-versioned', []),
                                 ('relink-versioned', ['--unknown']), ('release-stage', []),
                                 ('release-stage', ['bdb','--prefix=/usr','--libdir=/usr/lib','--dest=/']),
                                 ('release-arch', ['tkrzw','1','stage','out','--data-version=2']),
                                 ('check-pc-metadata', []), ('check-pc-metadata', ['--unknown'])]:
            with self.subTest(name=name, arguments=arguments):
                result = self.invoke(name, arguments)
                self.assertEqual(result.returncode, 2)
                if name == 'check-pc-metadata':
                    from tools.dev.packaging_templates import METADATA_USAGE
                    self.assertTrue(result.stderr.endswith(METADATA_USAGE.encode()))

    def test_missing_pin_and_command_execution_statuses(self):
        (self.root / 'tools/oracle/oracle-pin.txt').unlink()
        self.assertEqual(self.invoke('pc-version').returncode, 2)
        self.env['PATH'] = str(self.bin) + ':/usr/bin:/bin'
        (self.bin / 'cargo').chmod(0o644)
        args = ['libpinyin','--prefix=/usr','--destdir='+str(self.work/'stage')]
        self.assertEqual(self.invoke('install',args).returncode,126)
        (self.bin / 'cargo').unlink()
        args = ['libpinyin','--prefix=/usr','--destdir='+str(self.work/'stage')]
        self.assertEqual(self.invoke('install',args).returncode,127)

    def test_package_required_arguments_keep_status(self):
        for name in ('release-deb', 'release-rpm', 'release-arch'):
            result = self.invoke(name)
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

    def test_macos_install_keeps_the_cargo_c_library(self):
        self.fake('uname', "print('Darwin')")
        stage = self.work / 'stage'
        for library in packaging.LIBRARIES:
            with self.subTest(library=library):
                shutil.rmtree(stage, ignore_errors=True)
                result = self.invoke('install', [library, '--prefix=/usr', '--destdir=' + str(stage)],
                                     env={'CARGO_BUILD_TARGET': 'aarch64-apple-darwin'})
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual((stage / 'usr/lib' / (library + '.15.0.0.dylib')).read_bytes(), b'installed dylib')
                self.assertNotIn(b'relinked', result.stdout)


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
                self.assertEqual(self.invoke(name,args).returncode, expected)

    def test_installed_metadata_matches_and_reports_a_real_difference(self):
        result, stage = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        pin = self.work / 'pin'
        shutil.copytree(stage, pin)
        arguments = ['--pin-root=' + str(pin), '--subject-root=' + str(stage), '--prefix=/usr', '--modules=libpinyin']
        result = self.invoke('check-pc-metadata', arguments)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'OK: packaging metadata matches', result.stdout)
        path = stage / 'usr/lib/pkgconfig/libpinyin.pc'
        path.write_bytes(path.read_bytes().replace(b'BerkeleyDB', b'Tkrzw'))
        result = self.invoke('check-pc-metadata', arguments)
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
        result = self.invoke('release-deb', ['bdb','0.1.0',stage,'/usr/lib',out],
                              env={'PAYLOAD':str(payload)})
        self.assertEqual(result.returncode, 0, result.stderr)
        control = (payload / 'runtime/DEBIAN/control').read_text()
        self.assertIn('Depends: libc6 (>= 2.17), libglib2.0-0t64', control)
        self.assertIn('Provides: libpinyin15 (= 2.11.92), libzhuyin15 (= 2.11.92)', control)
        self.assertIn('Conflicts: libpinyin15, libzhuyin15', control)
        dev_control = (payload / 'dev/DEBIAN/control').read_text()
        self.assertIn('Depends: oxpinyin-libpinyin15-bdb (= 0.1.0-1), libglib2.0-dev\n', dev_control)
        result = self.invoke('release-deb', ['bdb','0.1.0',stage,'/usr/lib',out], env={'FAIL_PACKAGE':'1'})
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
        result = self.invoke('release-rpm', ['tkrzw','0.1.0',stage,out], env={'PAYLOAD':str(payload)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Provides:       libpinyin = 2.11.92 libzhuyin = 2.11.92', payload.read_text())
        self.assertIn('Obsoletes:      libpinyin libzhuyin', payload.read_text())
        result = self.invoke('release-rpm', ['tkrzw','0.1.0',stage,out], env={'PAYLOAD':str(payload),'NO_RPM':'1'})
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
        result = self.invoke('release-arch', ['kyotocabinet','0.1.0',stage,out,'--data='+str(data),'--data-version=2.10.3-2'], env={'PAYLOAD':str(payload)})
        self.assertEqual(result.returncode, 0, result.stderr)
        text = payload.read_text()
        self.assertIn('model data from Arch libpinyin 2.10.3-2', text)
        self.assertIn('libpinyin.so=15-64 libzhuyin.so=15-64', text)
        self.assertEqual(Path(str(payload)+'.data').read_bytes(), b'data')

    def test_linux_relink_keeps_versions_exports_z_defs_and_atomic_replacement(self):
        self.fake('uname', "print('Linux')")
        self.fake('cc', r"""import json,os,sys
from pathlib import Path
args=sys.argv[1:]
Path(os.environ['CALLS']).write_text(json.dumps(args))
Path(args[args.index('-o')+1]).write_bytes(b'linked')
sys.exit(int(os.environ.get('LINK_EXIT', '0')))
""")
        self.fake('nm', r"""import os
print('00000000 A LIBPINYIN')
print('00000001 T pinyin_init' + ('' if os.environ.get('UNVERSIONED') else '@@LIBPINYIN'))
if os.environ.get('EXTRA_EXPORT'): print('00000002 T leak@@LIBPINYIN')
""")
        self.fake('readelf', "import os; print('(SONAME) [' + os.environ.get('SONAME', 'libpinyin.so.15') + ']')")
        archive, ver, dest = [self.work / name for name in ('library.a', 'library.ver', 'libpinyin.so.15.0.0')]
        archive.write_bytes(b'archive')
        ver.write_text('LIBPINYIN {\n global:\n pinyin_init;\n local:\n *;\n};\n')
        arguments = ['--staticlib', archive, '--ver', ver, '--soname', 'libpinyin.so.15', '--dest', dest,
                     '--cc', self.bin / 'cc', '--features', 'kyotocabinet']
        for environment in ({}, {'LINK_EXIT': '7'}, {'EXTRA_EXPORT': '1'}, {'UNVERSIONED': '1'}, {'SONAME': 'wrong'}):
            with self.subTest(environment=environment):
                dest.write_bytes(b'original')
                result = self.invoke('relink-versioned', arguments, env=environment)
                self.assertEqual(result.returncode == 0, not environment, result.stderr)
                self.assertEqual(dest.read_bytes(), b'original' if environment else b'linked')
                args = json.loads((self.work / 'calls').read_text())
                self.assertEqual(args.count('-Wl,-z,defs'), 1)
                self.assertIn('-Wl,--version-script=' + str(ver), args)
                self.assertIn('-Wl,-soname,libpinyin.so.15', args)
                self.assertIn('-lstdc++', args)
                self.assertEqual(list(self.work.glob('*.relink.*')), [])

    def test_relink_cancellation_status_original_file_and_cleanup(self):
        self.fake('uname', "print('Linux')")
        archive, ver, dest = [self.work / name for name in ('library.a','library.ver','libpinyin.so.15.0.0')]
        archive.write_bytes(b'archive')
        ver.write_text('LIBPINYIN {\n global:\n pinyin_init;\n local:\n *;\n};\n')
        ready = self.work / 'ready'
        self.fake('cc', "import os,time; from pathlib import Path; Path(os.environ['READY']).write_text('ready'); time.sleep(.8)")
        arguments = ['--staticlib',str(archive),'--ver',str(ver),'--soname','libpinyin.so.15','--dest',str(dest)]
        for sig in (signal.SIGINT, signal.SIGTERM):
            dest.write_bytes(b'original')
            ready.unlink(missing_ok=True)
            command = [sys.executable,'-X','lazy_imports=normal','-c',DRIVER,str(self.root),'relink-versioned']
            with subprocess.Popen([*command,*arguments],cwd=ROOT,env={**self.env,'READY':str(ready)},
                                  stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True) as process:
                deadline = time.monotonic()+5
                while not ready.exists() and process.poll() is None and time.monotonic()<deadline:
                    time.sleep(.01)
                self.assertTrue(ready.exists(), 'compiler fixture did not start')
                process.send_signal(sig)
                process.communicate(timeout=10)
                self.assertEqual(process.returncode,128+sig)
            self.assertEqual(dest.read_bytes(),b'original')
            self.assertEqual(list(self.work.glob('*.relink.*')),[])

    def stage_fakes(self):
        self.env['CHECKOUT'] = str(self.root)
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
        result = self.invoke('release-stage',arguments)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn(b'release-stage gates passed (backend: bdb)',result.stdout)
        self.assertIn(b'pinyin=fixture',result.stdout)
        self.assertIn(b'zhuyin=fixture',result.stdout)
        result = self.invoke('release-stage',arguments,env={'LIBPINYIN_DATABASE_FORMAT':'Tkrzw'})
        self.assertEqual(result.returncode,1,result.stderr)
        self.assertIn(b'GATE FAILED: libpinyin database_format',result.stderr)


if __name__ == '__main__':
    unittest.main()
