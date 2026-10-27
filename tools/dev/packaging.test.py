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
triple=Path(value('--target', os.environ.get('CARGO_BUILD_TARGET','fixture-target'))).name.removesuffix('.json')
profile=value('--profile','release')
profile='debug' if profile=='dev' else profile
if '--debug' in args:profile='debug'
if '--release' in args:profile='release'
qualified=Path(value('--target-dir'))/triple/profile
qualified.mkdir(parents=True, exist_ok=True)
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
        self.fake('uname', "print('Darwin')")
        self.fake('rustc', "print('host: fixture-target')")

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

    def compare(self, name, args=(), *, env=None, reset=None, files=None, diagnostics=True):
        result = self.invoke(name, args, env=env)
        contents = tree(files) if files else None
        if BASE:
            if reset:
                reset()
            control = self.invoke(name, args, old=True, env=env)
            self.assertEqual(result.returncode, control.returncode)
            if diagnostics:
                self.assertEqual(self.normalize(result), self.normalize(control))
            if files:
                self.assertEqual(contents, tree(files))
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
        for text in ('libpinyin_tag=broken\n', 'no tag\n'):
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

    def test_explicit_layout_and_special_replacement_bytes(self):
        result, stage = self.install(extra=['--libdir=/usr/lib & hash#dir', '--includedir=/usr/inc\\lude#&'])
        self.assertEqual(result.returncode, 0, result.stderr)
        data = (stage / 'usr/lib & hash#dir/pkgconfig/libpinyin.pc').read_bytes()
        self.assertIn(b'libdir=/usr/lib & hash#dir\n', data)
        self.assertIn(b'includedir=/usr/inc\\lude#&\n', data)
        self.assertNotIn(os.fsencode(stage), data)

    def test_forwarding_custom_target_profile_and_empty_words(self):
        target = self.work / 'custom target'
        result, _ = self.install(extra=['--', '--target-dir', str(target), '--target=specs/fake.json',
                                          '--profile', 'dev', '--locked', '--features=a', '--features', 'b,c', ''])
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads((self.work / 'calls').read_text())
        self.assertEqual(sum(word.startswith('--target-dir=') for word in args), 1)
        self.assertIn('--target-dir=' + str(target), args)
        self.assertEqual(args[-1], '')
        self.assertTrue((target / 'fake/debug/libpinyin.pc.in.baked').is_file())

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


if __name__ == '__main__':
    unittest.main()
