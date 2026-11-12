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
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import packaging

DRIVER = '''import os, sys
from pathlib import Path
from tools.dev import packaging
class FixtureTools(packaging.Tools):
    def heavy(self, argv, **options):
        self.run(argv, **options)
    def load(self, path, symbol):
        return os.environ.get('FIXTURE_LOAD', '')
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
if triple.endswith('.json'):triple=triple[:-5]
profile=value('--profile','release')
profile='debug' if profile=='dev' else profile
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
    for key,val in [('VERSION','2.11.92'),('LIBPINYIN_BINARY_VERSION','15.0'),('LIBZHUYIN_BINARY_VERSION','15.0'),('DATABASE_FORMAT','BerkeleyDB')]:
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
if triple.endswith('-apple-darwin'):
    for suffix in ('.dylib','.15.dylib'):
        path=lib/(library+suffix)
        if path.is_symlink():path.unlink()
        path.symlink_to(library+'.15.0.0.dylib')
inc=Path(value('--destdir')+value('--includedir'))/'libpinyin-2.11.92'
inc.mkdir(parents=True,exist_ok=True)
for name in ['novel_types.h','pinyin_custom2.h', 'zhuyin_custom2.h' if library=='libzhuyin' else 'pinyin.h', 'zhuyin.h' if library=='libzhuyin' else 'pinyin.h']:
    shutil_source=Path.cwd()/name
    (inc/name).write_bytes(shutil_source.read_bytes())
'''


def tree(path):
    return {str(entry.relative_to(path)): ('link', os.readlink(entry)) if entry.is_symlink()
            else ('file', entry.read_bytes()) for entry in path.rglob('*') if not entry.is_dir()}


# The packaging fixtures run extensionless '#!python' stubs, chmod bits, make, relinks and
# POSIX signals; packaging and trainer-layout are refused on Windows, tested below.
if sys.platform != 'win32':
    class TrainerLayoutTests(unittest.TestCase):
        # Independent source inventory: trainer b1927376's subprocess calls plus
        # libpinyin 074a2219 data/Makefile.am:60-62. Not derived from TRAINER_TOOLS.
        REACHED = (
            'segment/ngseg', 'segment/spseg', 'segment/mergeseq',
            'storage/gen_binary_files', 'storage/import_interpolation',
            'training/gen_unigram', 'training/gen_k_mixture_model',
            'training/estimate_k_mixture_model', 'training/validate_k_mixture_model',
            'training/export_k_mixture_model', 'training/k_mixture_model_to_interpolation',
            'training/merge_k_mixture_model', 'training/prune_k_mixture_model',
            'training/estimate_interpolation', 'training/eval_correction_rate',
        )

        @classmethod
        def setUpClass(cls):
            metadata = json.loads(subprocess.check_output(
                ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT,
            ))
            package = next(package for package in metadata['packages']
                           if package['name'] == 'oxpinyin-utils')
            cls.bin_names = [target['name'] for target in package['targets']
                             if 'bin' in target['kind']]

        def setUp(self):
            temporary = tempfile.TemporaryDirectory(prefix='trainer-layout-')
            self.addCleanup(temporary.cleanup)
            self.work = Path(temporary.name)
            self.binaries = self.work / 'built tools'
            self.binaries.mkdir()
            self.target = self.work / 'trainer tree'
            self.tools = packaging.Tools(root=ROOT)
            for name in self.bin_names:
                path = self.binaries / name
                path.write_text('#!' + sys.executable + '\n' + '''import json, sys
from pathlib import Path
with Path('calls').open('a') as out:
    out.write(json.dumps([Path(sys.argv[0]).name, *sys.argv[1:]]) + '\\n')
if Path(sys.argv[0]).name == 'import_interpolation':
    Path('imported').write_bytes(sys.stdin.buffer.read())
''')
                path.chmod(0o755)

        def stage(self, target=None, binaries=None):
            return packaging.main(['trainer-layout', str(target or self.target), '--bin-dir',
                                   str(binaries or self.binaries)], tools=self.tools)

        def assert_inventory(self):
            for relative in self.REACHED:
                with self.subTest(path=relative):
                    path = self.target / 'utils' / relative
                    self.assertTrue(path.is_file())
                    self.assertTrue(os.access(path, os.X_OK))
                    self.assertFalse(path.is_symlink())

        def test_inventory_resolves_to_copied_executable_ports(self):
            self.assertEqual(self.stage(), 0)
            self.assert_inventory()
            # The test_* driver binaries are outside the trainer layout.
            ports = {name for name in self.bin_names if not name.startswith('test_')}
            installed = {path.name for path in (self.target / 'utils').glob('*/*')}
            self.assertEqual(installed, ports)
            # A Cargo clean must not invalidate a staged trainer tree.
            shutil.rmtree(self.binaries)
            self.assert_inventory()

        def test_existing_target_and_links_are_refused_without_writes(self):
            self.target.mkdir()
            (self.target / 'model').write_bytes(b'user data')
            before = tree(self.target)
            self.assertEqual(self.stage(), 2)
            self.assertEqual(tree(self.target), before)
            link = self.work / 'linked'
            link.symlink_to(self.target, target_is_directory=True)
            self.assertEqual(self.stage(target=link), 2)
            self.assertEqual(tree(self.target), before)
            dangling = self.work / 'dangling'
            dangling.symlink_to(self.work / 'absent', target_is_directory=True)
            self.assertEqual(self.stage(target=dangling), 2)
            self.assertFalse((self.work / 'absent').exists())

        def init_repository(self, worktree, *options):
            environment = {key: value for key, value in os.environ.items() if not key.startswith('GIT_')}
            subprocess.run(['git', 'init', '--quiet', *options, worktree],
                           env=environment, check=True, capture_output=True, timeout=20)

        def test_refuses_nested_and_symlinked_targets(self):
            checkout = self.work / 'checkout'
            self.init_repository(checkout)
            nested = checkout / 'nested'
            self.init_repository(nested)
            alias = self.work / 'alias'
            alias.symlink_to(nested, target_is_directory=True)
            for parent in (checkout, nested, alias):
                with self.subTest(parent=parent):
                    missing_parent = parent / 'not-created'
                    self.assertEqual(self.stage(target=missing_parent / 'trainer'), 2)
                    self.assertFalse(missing_parent.exists())
            self.assertEqual(self.stage(), 0)

        def test_refuses_gitfile_worktree(self):
            worktree = self.work / 'worktree'
            self.init_repository(worktree, '--separate-git-dir', self.work / 'repository')
            self.assertTrue((worktree / '.git').is_file())
            self.assertEqual(self.stage(target=worktree / 'trainer'), 2)
            self.assertFalse((worktree / 'trainer').exists())

        def test_refuses_symlink_git_entry(self):
            parent = self.work / 'linked entry'
            parent.mkdir()
            (parent / '.git').symlink_to(self.work / 'absent repository')
            self.assertEqual(self.stage(target=parent / 'trainer'), 2)
            self.assertFalse((parent / 'trainer').exists())

        def test_uninspectable_paths_are_refused_without_writes(self):
            blocker = self.work / 'regular file'
            blocker.write_bytes(b'not a directory')
            locked = self.work / 'locked'
            (locked / 'inner').mkdir(parents=True)
            locked.chmod(0)
            self.addCleanup(locked.chmod, 0o700)
            cases = {NotADirectoryError: blocker / 'missing' / 'trainer'}
            if os.geteuid():
                cases[PermissionError] = locked / 'inner' / 'trainer'
            for error, target in cases.items():
                with self.subTest(error=error.__name__):
                    with self.assertRaises(error):
                        target.lstat()
                    self.assertEqual(self.stage(target=target), 2)
            self.assertEqual(blocker.read_bytes(), b'not a directory')
            locked.chmod(0o700)
            self.assertEqual(list(locked.rglob('*')), [locked / 'inner'])

        def test_uninspectable_git_entry_is_refused_without_writes(self):
            lstat = Path.lstat
            for error in (PermissionError, NotADirectoryError):
                def refusing(path, error=error):
                    if path.name == '.git':
                        raise error(f'fixture refusal: {path}')
                    return lstat(path)
                with self.subTest(error=error.__name__), mock.patch.object(Path, 'lstat', refusing):
                    self.assertEqual(self.stage(), 2)
                self.assertFalse(self.target.exists())

        def test_missing_or_nonexecutable_tool_fails_before_creating_target(self):
            tool = self.binaries / 'gen_unigram'
            tool.chmod(0o644)
            self.assertEqual(self.stage(), 2)
            self.assertFalse(self.target.exists())
            tool.unlink()
            self.assertEqual(self.stage(), 2)
            self.assertFalse(self.target.exists())

        def make(self, *args):
            result = subprocess.run(['make', *args], cwd=self.target / 'data',
                                    capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            return result

        def test_make_build_clean_and_repeated_lambda_update(self):
            self.assertEqual(self.stage(), 0)
            data = self.target / 'data'
            model = b'caller-supplied interpolation model\n'
            (data / 'interpolation2.text').write_bytes(model)
            template = data / 'table.conf.in'
            self.assertEqual(template.read_bytes(), (ROOT / 'data/table.conf.in').read_bytes())
            self.assertNotRegex((data / 'Makefile').read_text(), r'\bgit\b')
            initial = template.read_text().replace('@DATABASE_FORMAT@', 'Tkrzw')
            (data / 'table.conf').write_text(initial)
            self.make()
            calls = [json.loads(line) for line in (data / 'calls').read_text().splitlines()]
            self.assertEqual(calls, [
                ['gen_binary_files', '--gen-punct-table', '--table-dir', '../data'],
                ['import_interpolation', '--table-dir', '../data'],
                ['gen_unigram', '--table-dir', '../data'],
            ])
            self.assertEqual((data / 'imported').read_bytes(), model)
            generated = ['bigram.db', 'phrase_index.bin', 'pinyin_index.bin',
                         'addon_phrase_index.bin', 'addon_pinyin_index.bin',
                         'gb_char.bin', 'gbk_char.bin', 'opengram.bin', 'merged.bin',
                         'art.bin', 'culture.bin', 'economy.bin', 'geology.bin', 'history.bin',
                         'life.bin', 'nature.bin', 'people.bin', 'science.bin', 'society.bin',
                         'sport.bin', 'technology.bin', 'punct.bin']
            preserved = ['deleted_bigram.db', 'estimates.db', 'evals2.text', 'unrelated.bin']
            for name in generated + preserved:
                (data / name).touch()
            self.make('clean')
            self.assertFalse(any((data / name).exists() for name in generated))
            self.assertTrue(all((data / name).is_file() for name in preserved))
            self.assertEqual((data / 'interpolation2.text').read_bytes(), model)
            self.assertEqual((data / 'table.conf').read_text(), initial)
            # A short/scientific lambda no longer matches the pin's pattern.
            # Repeated modify must still start from the original configuration.
            for value in ('1e-05', '0.4', '0.654321'):
                self.make('modify', 'LAMBDA_PARAMETER=' + value)
                self.assertEqual((data / 'table.conf').read_text(),
                                 initial.replace('0.312699', value))

        def test_real_just_recipe_transports_paths_with_spaces(self):
            result = subprocess.run(['just', 'trainer-layout', str(self.target), '--bin-dir',
                                     str(self.binaries)], cwd=ROOT, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_inventory()

        def check_real_tools(self):
            """Explicit integration test; needs built tools, fails if they are absent.

        Select TrainerLayoutTests.check_real_tools after building --bins.
        Ordinary harness discovery stays build-free (CI runs before tkrzw setup).
        OXPINYIN_TRAINER_BIN_DIR selects a nondefault Cargo output directory.
        """
            binaries = Path(os.environ.get('OXPINYIN_TRAINER_BIN_DIR', ROOT / 'target/debug'))
            self.assertEqual(self.stage(binaries=binaries), 0)
            self.assert_inventory()
            result = subprocess.run(['../utils/training/gen_unigram', '--help'],
                                    cwd=self.target / 'data', capture_output=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(b'Usage:', result.stdout)
            self.assertIn(b'--table-dir', result.stdout)


    class PackagingTests(unittest.TestCase):
        def setUp(self):
            self.temp = tempfile.TemporaryDirectory(prefix='packaging-test-')
            self.addCleanup(self.temp.cleanup)
            self.work = Path(self.temp.name)
            self.root = self.work / 'checkout'
            for relative in ['crates/oxpinyin-capi', 'crates/oxpinyin-zhuyin-capi', 'tools/oracle', 'tools/lib', 'tools/tkrzw']:
                (self.root / relative).mkdir(parents=True)
            for crate in packaging.LIBRARIES.values():
                for source in (ROOT / 'crates' / crate).iterdir():
                    if source.is_file():
                        shutil.copyfile(source, self.root / 'crates' / crate / source.name)
            shutil.copyfile(ROOT / 'tools/oracle/oracle-pin.txt', self.root / 'tools/oracle/oracle-pin.txt')
            # declared_default() reads the store manifest's [features].default from the checkout.
            (self.root / 'crates/oxpinyin-store').mkdir(parents=True)
            shutil.copyfile(ROOT / 'crates/oxpinyin-store/Cargo.toml', self.root / 'crates/oxpinyin-store/Cargo.toml')
            self.bin = self.work / 'bin'
            self.bin.mkdir()
            self.env = {**os.environ, 'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
                        'CALLS': str(self.work / 'calls'), 'STRIP_CALLS': str(self.work / 'strip-calls'), 'CARGO_BUILD_TARGET': 'fixture-target'}
            self.env.pop('CARGO_TARGET_DIR', None)
            self.fake('cargo', FAKE_CARGO)
            # The default fixture host needs no final relink.
            self.fake('uname', "print('fixture-host')")
            self.fake('rustc', "import os; print('host: ' + os.environ.get('FIXTURE_HOST', 'fixture-target'))")
            self.fake('dpkg', "print('fixture-architecture')")
            self.fake('nm','pass')
            self.fake('strip', "import json,os,sys; open(os.environ['STRIP_CALLS'],'a').write(json.dumps(sys.argv[1:])+'\\n')")
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

            shutil.copy(ROOT / 'tools/oracle/oracle-pin.txt', self.root / 'tools/oracle/oracle-pin.txt')
            self.fake('uv', 'import sys; from pathlib import Path; sys.argv[1:3] == ["python", "install"] and sys.exit(0); sys.path.insert(0, ' + repr(str(ROOT)) + '); from tools.dev.cell import main; sys.exit(main(["--shell"], root=Path.cwd()))')

        def fake(self, name, source):
            path = self.bin / name
            path.write_text('#!' + sys.executable + '\n' + source)
            path.chmod(0o755)

        def invoke(self, name, args=(), *, env=None):
            command = [sys.executable, '-X', 'utf8', '-c', DRIVER, str(self.root), name]
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

        def darwin_fakes(self):
            self.fake('uname', "print('Darwin')")
            self.env['CHECKOUT'] = str(self.root)
            self.env['LINK_CALLS'] = str(self.work / 'link-calls')
            self.env['DARWIN_EXPORTS'] = json.dumps({library: packaging.version_exports(
                self.root/'crates'/crate/(library+'.ver'))[1] for library, crate in packaging.LIBRARIES.items()})
            self.fake('lipo', "import os; print('x86_64' if os.environ.get('CARGO_BUILD_TARGET','').startswith('x86_64') else 'arm64')")
            self.fake('otool', r'''import os,sys
from pathlib import Path
library=Path(sys.argv[-1]).name.split('.15')[0]
name='/usr/lib/'+library+'.15.dylib'
if os.environ.get('BAD_IDENTITY') and '.relink.' in sys.argv[-1]:name='wrong'
print('Load command 1\n cmd LC_ID_DYLIB\n name '+name+' (offset 24)\n current version 15.2.3\n compatibility version 15.0.0')
print('Load command 2\n cmd LC_BUILD_VERSION\n platform 1\n minos 12.3\n sdk 27.2')
''')
            self.fake('cc', r'''import json,os,sys
from pathlib import Path
args=sys.argv[1:]
exports=Path(args[args.index('-exported_symbols_list')+2]).read_text()
Path(os.environ['LINK_CALLS']).write_text(json.dumps([args,exports]))
Path(args[args.index('-o')+1]).write_bytes(b'linked dylib')
sys.exit(int(os.environ.get('LINK_EXIT','0')))
''')
            for tool in ('nm', 'dyld_info'):
                self.fake(tool, r'''import json,os,sys
from pathlib import Path
library=Path(sys.argv[-1]).name.split('.15')[0]
names=set(json.loads(os.environ['DARWIN_EXPORTS'])[library])
if os.environ.get('MISSING_EXPORT'):names.remove(library.removeprefix('lib')+'_init')
if Path(sys.argv[0]).name=='nm':
    for name in sorted(names):print('10 T _'+name)
    if os.environ.get('NM_EXTRA'):print('20 D _dependency_data')
else:
    print('image [arm64]:\n -exports:\n offset symbol')
    for name in sorted(names):print('0x10 _'+name)
    if os.environ.get('TRIE_EXTRA'):print('0x20 _dependency_data')
''')

        def test_macos_relink_preserves_identity_exports_and_symlinks(self):
            self.darwin_fakes()
            stage = self.work / 'stage'
            for library in packaging.LIBRARIES:
                with self.subTest(library=library):
                    shutil.rmtree(stage, ignore_errors=True)
                    result = self.invoke('install', [library, '--prefix=/usr', '--destdir=' + str(stage)],
                                         env={'CARGO_BUILD_TARGET': 'aarch64-apple-darwin',
                                              'FIXTURE_HOST': 'aarch64-apple-darwin'})
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual((stage / 'usr/lib' / (library + '.15.0.0.dylib')).read_bytes(), b'linked dylib')
                    for suffix in ('.dylib', '.15.dylib'):
                        self.assertEqual((stage/'usr/lib'/(library+suffix)).readlink(), Path(library+'.15.0.0.dylib'))
                    self.assertIn(b'relinked', result.stdout)
                    self.assertIn(b'loaded ', result.stdout)
                    calls, exports = json.loads((self.work / 'link-calls').read_text())
                    self.assertEqual(calls[calls.index('-install_name')+1], '/usr/lib/'+library+'.15.dylib')
                    self.assertEqual(calls[calls.index('-arch')+1], 'arm64')
                    self.assertIn('-mmacosx-version-min=12.3', calls)
                    self.assertIn('-Wl,-current_version,15.2.3', calls)
                    self.assertIn('-Wl,-compatibility_version,15.0.0', calls)
                    self.assertEqual(calls.count('-exported_symbols_list'), 1)
                    self.assertIn('-force_load', calls)
                    names = json.loads(self.env['DARWIN_EXPORTS'])[library]
                    self.assertEqual(exports.splitlines(), ['_'+name for name in names])
                    self.assertIn('_'+library.removeprefix('lib')+'_init\n', exports)
                    self.assertNotIn('LIBPINYIN', exports)
                    self.assertNotIn('oxpinyin_', exports)
                    self.assertFalse((self.work / 'strip-calls').exists())
                    self.assertEqual(list((stage/'usr/lib').glob('*.relink.*')), [])

        def test_macos_dsym_consumes_retained_lto_object_before_cleanup(self):
            self.darwin_fakes()
            self.fake('xcrun', r'''import json, os, sys
from pathlib import Path
args=sys.argv[1:]
assert args[0]=='dsymutil'
commands=json.loads(Path(os.environ['LINK_CALLS']).read_text())[0]
option=next(word for word in commands if word.startswith('-Wl,-object_path_lto,'))
assert Path(option.split(',',2)[2]).parent.exists()
assert Path(args[1]).exists()
Path(args[args.index('-o')+1]).mkdir()
''')
            result, stage = self.install(env={'CARGO_BUILD_TARGET':'aarch64-apple-darwin',
                'FIXTURE_HOST':'aarch64-apple-darwin', 'OXPINYIN_DSYM':'1'})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((stage/'usr/lib/libpinyin.15.0.0.dylib.dSYM').is_dir())
            self.assertEqual(list((stage/'usr/lib').glob('*.relink.*')), [])

        def test_macos_relink_rejects_bad_exports_or_identity_before_replacement(self):
            self.darwin_fakes()
            for environment, diagnostic in [({'LINK_EXIT':'7'}, ''), ({'NM_EXTRA':'1'}, 'extra:'),
                                            ({'TRIE_EXTRA':'1'}, 'extra:'), ({'MISSING_EXPORT':'1'}, 'missing:'),
                                            ({'BAD_IDENTITY':'1'}, 'identity differs')]:
                with self.subTest(environment=environment):
                    result, stage = self.install(env={**environment, 'CARGO_BUILD_TARGET':'aarch64-apple-darwin',
                                                     'FIXTURE_HOST':'aarch64-apple-darwin'})
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(diagnostic.encode(), result.stderr)
                    self.assertEqual((stage/'usr/lib/libpinyin.15.0.0.dylib').read_bytes(), b'installed dylib')
                    self.assertEqual(list((stage/'usr/lib').glob('*.relink.*')), [])

        def test_macos_install_fails_when_the_dylib_does_not_load(self):
            self.darwin_fakes()
            result = self.invoke('install', ['libpinyin', '--prefix=/usr', '--destdir=' + str(self.work / 'stage')],
                                 env={'CARGO_BUILD_TARGET': 'aarch64-apple-darwin',
                                      'FIXTURE_HOST': 'aarch64-apple-darwin', 'FIXTURE_LOAD': 'fixture load failure'})
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'does not load', result.stderr)
            self.assertIn(b'fixture load failure', result.stderr)

        def test_macos_install_skips_the_load_check_for_a_non_native_target(self):
            self.darwin_fakes()
            stage = self.work / 'stage'
            result = self.invoke('install', ['libpinyin', '--prefix=/usr', '--destdir=' + str(stage)],
                                 env={'CARGO_BUILD_TARGET': 'x86_64-apple-darwin',
                                      'FIXTURE_HOST': 'aarch64-apple-darwin', 'FIXTURE_LOAD': 'fixture load failure'})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(b'load check skipped: target x86_64-apple-darwin is not the running architecture '
                          b'(aarch64-apple-darwin)', result.stdout + result.stderr)
            self.assertNotIn(b'does not load', result.stderr)
            self.assertFalse((self.work / 'strip-calls').exists())

        def test_load_checks_the_image_in_a_child_without_dyld_overrides(self):
            source = self.work / 'probe.c'
            source.write_text('int probe_entry(void) { return 7; }\n')
            library = self.work / 'libprobe.dylib'
            subprocess.run(['cc', '-shared', '-fPIC', '-o', str(library), str(source)], check=True)
            env = {**os.environ, 'DYLD_LIBRARY_PATH': '/nonexistent', 'DYLD_INSERT_LIBRARIES': '/nonexistent/x.dylib'}
            tools = packaging.Tools(env=env)
            self.assertEqual(tools.load(library, 'probe_entry'), '')
            link = self.work / 'link.dylib'
            link.symlink_to(library)
            self.assertEqual(tools.load(link, 'probe_entry'), '')
            self.assertIn('probe_no_such', tools.load(library, 'probe_no_such'))
            broken = self.work / 'broken.dylib'
            broken.write_bytes(b'installed dylib')
            self.assertNotEqual(tools.load(broken, 'probe_entry'), '')
            import ctypes.util
            mismatch = tools.load(ctypes.util.find_library('c'), 'strlen')
            self.assertIn('resolved in', mismatch)


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

        def test_relative_staging_roots_are_refused_before_removal(self):
            # Relative paths resolve from the repository root, so 'target' would name Cargo's target directory.
            sentinel = self.root / 'target/keep'
            for name, arguments in [('release-stage', ['bdb', '--prefix=/usr', '--libdir=/usr/lib', '--dest=target']),
                                    ('check-pc-metadata', ['--pin-root=' + str(self.work), '--subject-root=target',
                                                           '--prefix=/usr', '--build-subject=bdb'])]:
                with self.subTest(name=name):
                    sentinel.parent.mkdir(parents=True, exist_ok=True)
                    sentinel.write_text('kept')
                    result = self.invoke(name, arguments)
                    self.assertEqual(result.returncode, 2, result.stderr)
                    self.assertIn(b"'target' must be an absolute path", result.stderr)
                    self.assertTrue(sentinel.is_file())

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
            arguments = ['--staticlib',str(archive),'--ver',str(ver),'--soname','libpinyin.so.15','--dest',str(dest),'--backend','bdb']
            for sig in (signal.SIGINT, signal.SIGTERM):
                dest.write_bytes(b'original')
                ready.unlink(missing_ok=True)
                command = [sys.executable,'-X','utf8','-c',DRIVER,str(self.root),'relink-versioned']
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

        def test_release_stage_complete_tree(self):
            self.stage_fakes()
            stage = self.work / 'stage'
            arguments = ['bdb','--prefix=/usr','--libdir=/usr/lib','--dest='+str(stage)]
            result = self.invoke('release-stage',arguments)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertIn(b'release-stage gates passed (backend: bdb)',result.stdout)
            self.assertIn(b'pinyin=fixture',result.stdout)
            self.assertIn(b'zhuyin=fixture',result.stdout)


if sys.platform == 'win32':
    class WindowsRefusalTests(unittest.TestCase):
        """The recipes the maintainers keep Linux-only: real just, no command mocks."""

        def test_real_just_refuses_packaging_and_trainer_layout(self):
            for recipe, arguments in [('packaging', ['install', 'libpinyin']),
                                      ('trainer-layout', ['target', '--bin-dir', 'built tools'])]:
                with self.subTest(recipe=recipe):
                    # pwsh is the supported interactive caller; avoid legacy argument conversion.
                    command = ' '.join("'" + value.replace("'", "''") + "'" for value in
                                       ['--justfile', str(ROOT / 'justfile'), recipe, *arguments])
                    result = subprocess.run(['pwsh', '-NoLogo', '-NoProfile', '-Command',
                                             'just ' + command + '; exit $LASTEXITCODE'],
                                            cwd=ROOT, capture_output=True, text=True, timeout=30)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn(recipe + ' is unsupported on Windows: ', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
