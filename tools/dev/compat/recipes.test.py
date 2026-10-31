#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Oracle controller fixtures with fake host adapters."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import pool, slot
from tools.dev.compat import campaigns, commands, fixtures, identity, images, launcher, process
from tools.dev.runners import Finished


class RecipeTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.directory = Path(directory.name)
        self.env = dict(os.environ, UV_NO_CACHE='1')
        for key in tuple(self.env):
            if key in launcher.boundary.STATE_SETTINGS or key.startswith(('OXPINYIN_', 'PINYIN_', 'ORACLE_', 'UV_PYTHON_')):
                self.env.pop(key)
        # A forwarding uv exercises the actual modules without uv's own version diagnostics.
        uv = self.directory / 'uv'
        uv.write_text(f'#!{sys.executable}\nimport os,sys\na=sys.argv[1:]\ni=a.index("python"); os.execv(sys.executable,[sys.executable,*a[i+1:]])\n')
        uv.chmod(0o755)
        self.env['PATH'] = str(self.directory) + os.pathsep + self.env['PATH']

    def invoke(self, recipe, args):
        program = "import os,sys,threading\nfrom tools.dev.compat import commands,fixtures,images\nfrom tools.dev import pool\nif os.environ.get('FAKE_CAMPAIGN'):\n    import subprocess\n    from tools.dev.compat import process\n    process.slot.run_command=lambda command,env=None,cwd=None,stdout=None,**options: subprocess.run(command,env=env,cwd=cwd,stdout=stdout).returncode\nif os.environ.get('FAKE_INTERCHANGE'):\n    original=fixtures.run\n    def intercepted(command,**options):\n        if 'system-import-round-trip' in command:\n            print('system-import: OK'); return int(os.environ.get('FAKE_INTERCHANGE_STATUS','0'))\n        return original(command,**options)\n    fixtures.run=intercepted\nif sys.argv[1:2]==['reproducible']:\n    def admitted(units,*args,**kwargs):\n        return [function(pool.UnitContext(name,threading.Event(),environment=dict(os.environ))) for name,function in units]\n    images.pool.run_admitted=admitted\nraise SystemExit(commands.main(sys.argv[1:]))\n"
        current = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', program, *recipe, *args],
                                 cwd=ROOT, env=self.env, capture_output=True, timeout=20)
        return current

    def test_compat_first_pass_argument_statuses(self):
        for args, code in (([], 2), (['--cell', 'invalid', '--list'], 2), (['--jobs', '0', '--list'], 2),
                           (['--memory-mib', '00', '--list'], 2), (['--runners', ', ,'], 2),
                           (['--unknown'], 2), (['--runtime'], 2), (['--memory-smoke', '--runners', 'bisect'], 1),
                           (['--help'], 0)):
            with self.subTest(args=args):
                self.assertEqual(self.invoke([], args).returncode, code)

    def test_compat_shell_passes_prefixes_through_shared_cli_and_launcher(self):
        uv = self.directory / 'uv'
        uv.write_text(f'''#!{sys.executable}
import json,sys
sys.path.insert(0, {str(ROOT)!r})
from tools.dev import cli
from tools.dev.compat import launcher
args = sys.argv[sys.argv.index('-m') + 2:]
def launch(argv):
    print(json.dumps(launcher.arguments(argv)[1]))
    return 0
launcher.launch = launch
raise SystemExit(cli.main(args))
''')
        # The shell runs this executable at the checkout root.
        env = dict(self.env)
        env.pop('UV_CACHE_DIR', None)
        result = subprocess.run(['bash', 'tools/oracle/compat.sh', '--runners', 'contract-diff',
                                 '--cases', 'default-option-parse',
                                 '--cases-prefix', 'table-conf-,user-conf-'],
                                cwd=ROOT, env=env, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), ['--runners', 'contract-diff', '--cases',
                                                    'default-option-parse', '--cases-prefix',
                                                    'table-conf-,user-conf-'])

    def test_developer_input_error_statuses(self):
        for args in ([], ['--unknown'], ['--libpinyin', '/absent']):
            self.assertEqual(self.invoke(['differentials'], args).returncode, 2)
        done = self.invoke(['user-round-trip'], ['/missing-prefix'])
        self.assertEqual(done.returncode, 77)
        self.assertEqual(self.invoke(['drop-in'], ['invalid', str(self.directory)]).returncode, 2)

    def test_image_options_and_invalid_platform_statuses(self):
        for recipe in ('verify-image', 'reproducible'):
            for args in (['--unknown'], ['--platform', 'linux/riscv64', 'image'] if recipe == 'verify-image'
                         else ['--platform', 'linux/riscv64']):
                self.assertEqual(self.invoke([recipe], args).returncode, 2)
        self.assertEqual(self.invoke(['verify-image'], []).returncode, 2)

    def test_reference_tag_keeps_its_original_input_order_and_digest(self):
        result = self.invoke(['image-tag'], [])
        self.assertEqual(result.stdout.decode().strip(), images.image_tag())
        before = images.image_tag()
        copied = self.directory / 'tree'
        for path in ('tools/oracle/Dockerfile.oracle','tools/oracle/build-oracle.sh','tools/oracle/version-manifest.json',
                     'tools/oracle/versions.py','tools/tkrzw/distro-probe.sh','tools/oracle/oracle-pin.txt'):
            dest = copied / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / path, dest)
        self.assertEqual(images.image_tag(copied), before)
        (copied / 'tools/oracle/version-manifest.json').write_bytes(b'changed input')
        self.assertNotEqual(images.image_tag(copied), before)

    def test_platform_pin_selection_moves_the_shell_regressions(self):
        pin = ROOT / 'tools/oracle/oracle-pin.txt'
        amd = images.platform_field(pin, 'linux/amd64', 'libpinyin_sha256_bdb')
        arm = images.platform_field(pin, 'linux/arm64', 'libpinyin_sha256_bdb')
        self.assertRegex(amd, '^[0-9a-f]{64}$')
        self.assertRegex(arm, '^[0-9a-f]{64}$')
        self.assertNotEqual(amd, arm)
        self.assertEqual(images.platform_field(pin, 'linux/riscv64', 'libpinyin_sha256_bdb'), '')
        for cell in ('bdb', 'kc', 'tkrzw'):
            for field in ('libpinyin_sha256_', 'libzhuyin_sha256_', 'data_manifest_sha256_'):
                self.assertRegex(images.platform_field(pin, 'linux/arm64', field + cell), '^[0-9a-f]{64}$')

    def test_pin_field_reads_use_platform_fields_without_fallback(self):
        # Platform fields do not fall back to unqualified fields; missing fields
        # print nothing and unsupported platforms fail silently.
        pin = ROOT / 'tools/oracle/oracle-pin.txt'
        text = pin.read_text()

        def current(platform, key):
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                status = commands.main(['pin-field', '--platform', platform, key])
            return status, out.getvalue()

        keys = ('platform', 'libpinyin_commit', 'base_image', 'absent_field', 'libpinyin_sha256_bdb',
                'libzhuyin_sha256_tkrzw', 'data_manifest_sha256_kc')
        for platform, suffix in (('linux/amd64', ''), ('linux/arm64', '_linux_arm64')):
            for key in keys:
                with self.subTest(platform=platform, key=key):
                    match = re.search('^' + re.escape(key + suffix) + '=(.*)$', text, re.M)
                    expected = (0, match.group(1) + '\n' if match else '')
                    self.assertEqual(current(platform, key), expected)
        self.assertEqual(current('linux/amd64', 'platform'), (0, 'linux/amd64\n'))
        self.assertEqual(current('linux/arm64', 'platform'), (0, 'linux/arm64\n'))
        self.assertEqual(current('linux/arm64', 'libpinyin_commit'), (0, ''))
        status, output = current('linux/riscv64', 'libpinyin_sha256_bdb')
        self.assertEqual((status != 0, output), (True, ''))

    def test_staged_guest_is_bound_to_the_snapshot_and_input_identity(self):
        directory=self.directory / 'input'
        directory.mkdir()
        with tarfile.open(directory / 'source.tar','w') as archive:
            for source,target in launcher.STAGED.items():
                path=ROOT / source
                archive.add(path,arcname=source)
                shutil.copyfile(path,directory / target)
        (directory / 'model.tar.gz').touch()
        digest=identity.seal_inputs(directory,'a'*16,'reference','head')
        record=identity.verify_inputs(directory,digest)
        self.assertEqual(record['guest_sha256'],identity.digest_file(directory / 'guest.py'))
        (directory / 'guest.py').write_text('tampered helper')
        with self.assertRaisesRegex(ValueError,'content mismatch: guest.py'):
            identity.verify_inputs(directory,digest)
        with self.assertRaisesRegex(ValueError,'launch helper differs from source snapshot: tools/dev/compat/guest.py'):
            identity.seal_inputs(directory,'a'*16,'reference','head')

    def test_image_identity_is_the_original_byte_stream(self):
        digest = hashlib.sha256()
        for path in ('tools/oracle/Dockerfile.compat', 'rust-toolchain.toml', '.python-version', 'tools/install-python.sh', 'tools/lib/bash-floor.sh'):
            digest.update((ROOT / path).read_bytes())
        digest.update(b'reference:no-custom-ca:linux/amd64')
        self.assertEqual(launcher.image_identity(ROOT, 'reference', 'no-custom-ca', 'linux/amd64'), digest.hexdigest()[:16])

    def test_image_copy_failure_keeps_child_status_and_removes_only_its_container(self):
        engine = self.directory / 'docker'
        log = self.directory / 'calls'
        engine.write_text(f'''#!{sys.executable}
import json,pathlib,sys
with pathlib.Path({str(log)!r}).open('a') as f: f.write(json.dumps(sys.argv[1:])+"\\n")
if sys.argv[1]=='create': print('owned-id')
if sys.argv[1]=='cp': sys.exit(19)
''')
        engine.chmod(0o755)
        args = ['--engine', str(engine), 'image']
        self.assertEqual(self.invoke(['verify-image'], args).returncode, 19)
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertTrue(all(row == ['rm', '-f', 'owned-id'] for row in rows if row[0] == 'rm'))

    def test_capture_refuses_missing_oracle_without_writing_outputs(self):
        output = self.directory / 'output'
        done = self.invoke(['capture'], [str(self.directory), str(output)])
        self.assertEqual(done.returncode, 1)
        self.assertFalse(output.exists())

    def test_capture_rejects_wrong_schema_and_never_regenerates_fixtures(self):
        prefix = self.directory / 'prefix'
        for name in ('include/libpinyin-2.11.92/pinyin.h', 'lib/libpinyin.so.15.0.0', 'oracle-data.sha256'):
            path = prefix / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        (prefix / 'lib/libpinyin/data').mkdir(parents=True)
        (prefix / 'oracle-pin.txt').write_text('schema=wrong\n')
        output = self.directory / 'output'
        done = self.invoke(['capture'], [str(prefix), str(output)])
        self.assertEqual(done.stderr, b'unsupported oracle manifest schema\n')
        self.assertFalse(output.exists())

    def test_heavy_launch_uses_the_existing_slot_and_launch_statuses(self):
        with mock.patch.object(process.slot, 'run_command', return_value=-15) as launch:
            self.assertEqual(process.run(['cargo', 'test'], heavy=True, check=False), 143)
        self.assertEqual(launch.call_count, 1)
        for error, expected in ((FileNotFoundError(2, 'missing'), 127), (PermissionError(13, 'denied'), 126)):
            with mock.patch.object(process.subprocess, 'Popen', side_effect=error), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(process.run(['command'], check=False), expected)

    def test_aggregate_suite_failure_keeps_later_suites_running(self):
        with mock.patch.object(campaigns, 'run', side_effect=[7, 0]) as child, contextlib.redirect_stdout(io.StringIO()) as output:
            failed = campaigns.run_suite('first', ['cargo'], env={})
            failed |= campaigns.run_suite('second', ['cargo'], env={})
        self.assertTrue(failed)
        self.assertEqual(child.call_count, 2)
        self.assertIn('FAILED: first', output.getvalue())
        self.assertNotIn('FAILED: second', output.getvalue())

    def test_nightly_build_and_run_halves_are_separable(self):
        work = self.directory / 'work'
        with mock.patch.object(campaigns, 'build_pin') as build, mock.patch.object(campaigns, 'run_fixtures', return_value=5) as suites:
            self.assertEqual(campaigns.nightly(['build', '--deps-image', str(work)]), 0)
            build.assert_called_once_with(work, root=process.ROOT, in_image=True)
            suites.assert_not_called()
            build.reset_mock()
            self.assertEqual(campaigns.nightly(['run', str(work)]), 5)
            build.assert_not_called()
            suites.assert_called_once_with(work, root=process.ROOT)
            suites.reset_mock()
            self.assertEqual(campaigns.nightly([str(work)]), 5)
            build.assert_called_once_with(work, root=process.ROOT, in_image=False)
            suites.assert_called_once_with(work, root=process.ROOT)
            with self.assertRaises(Finished) as refused, contextlib.redirect_stderr(io.StringIO()):
                campaigns.nightly(['run', '--deps-image', str(work)])
            self.assertEqual(refused.exception.status, 2)

    def test_nightly_run_refuses_a_tree_that_is_not_the_pin(self):
        (self.directory / 'libpinyin').mkdir()
        (self.directory / 'libpinyin/.pin-ok').write_text('0' * 40 + '\n')
        with mock.patch.object(campaigns, 'run') as child, self.assertRaises(Finished) as refused, \
                contextlib.redirect_stderr(io.StringIO()) as error:
            campaigns.run_fixtures(self.directory)
        self.assertEqual(refused.exception.status, 2)
        self.assertIn('run nightly-fixtures build first', error.getvalue())
        child.assert_not_called()

    def test_nightly_loader_check_refuses_a_pin_tool_on_another_libtkrzw(self):
        def check(cell, runner, side, obj, *, env):
            if Path(obj).name == 'ngseg':
                raise campaigns.runtime.RuntimeRefused(f'{runner}: {side}: libtkrzw resolved to /usr/lib/libtkrzw.so.1')
            return None if Path(obj).name == 'gen_unigram' or side == 'subject' else Path('/verified/lib/libtkrzw.so.1')
        with mock.patch.object(campaigns.runtime, 'check', side_effect=check) as checked, \
                contextlib.redirect_stdout(io.StringIO()) as output, contextlib.redirect_stderr(io.StringIO()), \
                self.assertRaises(Finished):
            campaigns.certify_tkrzw(self.directory / 'src', self.directory / 'prefix', {}, [self.directory / 'subject'])
        self.assertEqual(checked.call_count, 1 + len(campaigns.NIGHTLY_INSTALLED) + len(campaigns.NIGHTLY_TOOLS) + 1)
        self.assertIn(self.directory / 'src/utils/segment/ngseg', [call.args[3] for call in checked.call_args_list])
        self.assertIn('pin: libtkrzw resolved to /usr/lib/libtkrzw.so.1', output.getvalue())
        self.assertIn('gen_unigram: libtkrzw.so.1 => not loaded', output.getvalue())
        self.assertIn('ldd: subject', output.getvalue())

    def test_nightly_loader_check_passes_when_every_loaded_libtkrzw_is_verified(self):
        verified = Path('/verified/lib/libtkrzw.so.1')
        with mock.patch.object(campaigns.runtime, 'check', return_value=verified), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            campaigns.certify_tkrzw(self.directory / 'src', self.directory / 'prefix', {}, [])
        self.assertNotIn('REFUSED', output.getvalue())
        self.assertIn(f'libpinyin.so: libtkrzw.so.1 => {verified}', output.getvalue())


class InterchangeTests(unittest.TestCase):
    """Complete capture and interchange paths with fake host adapters."""
    setUp = RecipeTests.setUp
    invoke = RecipeTests.invoke

    def tools(self):
        driver = f'''#!{sys.executable}
import os,pathlib,sys
if len(sys.argv)>1 and sys.argv[1] in ('F-A','F-C','ZH'):
    print(sys.argv[1],sys.argv[-1]); raise SystemExit(0)
action=sys.argv[1]; profile=pathlib.Path(sys.argv[3])
if action=='train':
    if os.environ.get('FAKE_PIN_STATUS'): raise SystemExit(int(os.environ['FAKE_PIN_STATUS']))
    (profile/'trained').write_text('private profile')
elif action=='dump':
    if not os.environ.get('FAKE_EMPTY'):
        if not os.environ.get('FAKE_NO_BIGRAM'): print('B\\tgrams')
        print('P\\t你好')
elif action=='phrases': print('P\\t你好')
'''
        cc = self.directory / 'cc'
        cc.write_text(f'''#!{sys.executable}
import pathlib,sys
out=pathlib.Path(sys.argv[sys.argv.index('-o')+1]); out.write_text({driver!r}); out.chmod(0o755)
''')
        cc.chmod(0o755)
        for name in ('pkg-config', 'ldd'):
            path = self.directory / name
            path.write_text('#!/bin/sh\nexit 0\n')
            path.chmod(0o755)
        subject = self.directory / 'subject-test'
        subject.write_text(f'''#!{sys.executable}
import os,pathlib,sys
print('test:'+sys.argv[-1])
if os.environ.get('FAKE_SUBJECT_STATUS'): raise SystemExit(int(os.environ['FAKE_SUBJECT_STATUS']))
if 'OX_EXPECTED_PHRASES' in os.environ: pathlib.Path(os.environ['OX_EXPECTED_PHRASES']).write_text('你好\\n')
if 'OX_PIN_DIR' in os.environ: assert (pathlib.Path(os.environ['OX_PIN_DIR'])/'trained').is_file()
''')
        subject.chmod(0o755)
        return subject

    def prefix(self, capture=False):
        prefix = self.directory / 'prefix with space'
        header = prefix / 'include/libpinyin-2.11.92/pinyin.h'
        header.parent.mkdir(parents=True)
        header.write_bytes(b'header')
        library = prefix / 'lib/libpinyin.so.15.0.0'
        library.parent.mkdir()
        library.write_bytes(b'library')
        for name in ('libpinyin.so', 'libpinyin.so.15'):
            (library.parent / name).symlink_to(library.name)
        data = prefix / 'lib/libpinyin/data'
        data.mkdir(parents=True)
        (data / 'table.conf').write_bytes(b'table')
        stable = prefix / 'oracle-data.sha256'
        stable.write_text(hashlib.sha256(b'table').hexdigest() + '  lib/libpinyin/data/table.conf\n')
        record = dict(schema='pinyin-oracle-v1', pin_ref=fixtures.CAPTURE_PIN, dbm='Tkrzw' if capture else 'BerkeleyDB',
                      header_sha256=identity.digest_file(header), shared_object_sha256=identity.digest_file(library),
                      data_manifest_sha256=identity.digest_file(stable))
        (prefix / 'oracle-pin.txt').write_text(''.join(f'{key}={value}\n' for key,value in record.items()))
        return prefix

    def test_capture_formats_and_payload_checks(self):
        self.tools()
        prefix = self.prefix(capture=True)
        output = self.directory / 'captured'
        paths = [output / name for name in ('f-a.txt','f-c.txt','zhuyin-parse.txt')]
        result = self.invoke(['capture'], [str(prefix), str(output)])
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        for family,path in zip(('F-A','F-C','ZH'),paths):
            self.assertEqual(path.read_text(), family+' '+fixtures.CAPTURE_PIN+'\n')
        (prefix / 'lib/libpinyin/data/table.conf').write_bytes(b'tampered')
        result = self.invoke(['capture'], [str(prefix), str(output)])
        self.assertEqual(result.returncode, 1)
        self.assertIn(b'oracle data payload checksum mismatch',result.stderr)

    def test_both_interchange_directions_and_child_failures(self):
        subject = self.tools()
        prefix = self.prefix()
        self.env.update(OXPINYIN_USER_RT_TEST=str(subject), OXPINYIN_CAPI_SO=str(prefix / 'lib/libpinyin.so'), FAKE_INTERCHANGE='1')
        args = [str(prefix), 'nihao', 'nisha', 'nihaoa']
        result = self.invoke(['user-round-trip'], args)
        self.assertEqual(result.returncode,0,result.stderr.decode())
        self.assertIn(b'IDENTICAL:',result.stdout)
        self.assertIn(b'READABLE:',result.stdout)
        self.assertIn(b'system-import: OK',result.stdout)
        for key,code in (('FAKE_PIN_STATUS',19),('FAKE_SUBJECT_STATUS',17),('FAKE_INTERCHANGE_STATUS',23)):
            with self.subTest(key=key):
                self.env[key]=str(code)
                self.assertEqual(self.invoke(['user-round-trip'],args).returncode,code)
                self.env.pop(key)
        for key in ('FAKE_EMPTY','FAKE_NO_BIGRAM'):
            self.env[key]='1'
            self.assertEqual(self.invoke(['user-round-trip'],args).returncode,1)
            self.env.pop(key)

    def test_developer_campaign_wiring_reports_and_aggregate_failures(self):
        library = self.directory / 'libpinyin'
        data,exported,model = (self.directory / name for name in ('data','export','model'))
        for path in (library / 'src/.libs/libpinyin.so.15', data / 'table.conf', data / 'phrase_index.bin',
                     data / 'pinyin_index.bin', model / 'interpolation2.text'):
            path.parent.mkdir(parents=True,exist_ok=True)
            path.touch()
        exported.mkdir()
        (exported / 'datagen-manifest.txt').write_text('backend=tkt\n')
        for relative in campaigns.TOOLS.values():
            path=library / relative
            path.parent.mkdir(parents=True,exist_ok=True)
            path.write_text('#!/bin/sh\nexit 0\n')
            path.chmod(0o755)
        cargo=self.directory / 'cargo'
        cargo.write_text(f"#!{sys.executable}\nimport os,sys\nprint('not in the report')\nprint('live parity: '+sys.argv[sys.argv.index('--test')+1])\nprint('test result: fixture',file=sys.stderr)\nif os.environ.get('FAKE_SUITE_FAILURE')==sys.argv[sys.argv.index('--test')+1]: raise SystemExit(7)\n")
        cargo.chmod(0o755)
        self.env['FAKE_CAMPAIGN']='1'
        args=['--libpinyin',str(library),'--data',str(data),'--export',str(exported),'--model',str(model)]
        self.assertEqual(self.invoke(['differentials'],args).returncode,0)
        self.env['FAKE_SUITE_FAILURE']='cross_backend'
        result=self.invoke(['differentials'],args)
        self.assertEqual(result.returncode,1)
        self.assertIn(b'FAILED: datagen:',result.stdout)
        self.assertIn(b'counter: gen_ngram',result.stdout)
        self.assertNotIn(b'not in the report',result.stdout)

    def test_reproducibility_hashes_and_aggregate_failures(self):
        engine = self.directory / 'docker'
        engine.write_text(f'''#!{sys.executable}
import os,pathlib,sys
a=sys.argv[1:]
i=a.index('--'); dbm,work,prefix,jobs=a[i+1:i+5]
record=dict(line.split('=',1) for line in pathlib.Path(os.environ['FAKE_PIN_PATH']).read_text().splitlines() if line and not line.startswith('#'))
for lib in ('libpinyin','libzhuyin'):
    checksum=record[lib+'_sha256_'+dbm]
    if os.environ.get('FAKE_HASH_DIFFER')==dbm and '/oracle-b/' in prefix: checksum='0'*64
    print(lib,checksum)
if os.environ.get('FAKE_BUILD_FAILURE')==dbm and '/oracle-a/' in prefix: raise SystemExit(29)
''')
        engine.chmod(0o755)
        self.env['FAKE_PIN_PATH']=str(ROOT / 'tools/oracle/oracle-pin.txt')
        args=['--engine',str(engine),'--image','fixture','--jobs','2']
        self.assertEqual(self.invoke(['reproducible'],args).returncode,0)
        for key in ('FAKE_HASH_DIFFER','FAKE_BUILD_FAILURE'):
            self.env[key]='kc'
            result=self.invoke(['reproducible'],args)
            self.assertEqual(result.returncode,1)
            self.assertIn(b'FAIL: kc',result.stderr)
            self.assertIn(b'tkrzw libzhuyin',result.stdout)
            self.env.pop(key)

if __name__ == '__main__':
    unittest.main()
