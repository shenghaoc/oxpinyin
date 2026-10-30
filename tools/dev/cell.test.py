#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Portable cell settings, loader paths, atomic ownership and build admission tests."""
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.dev import host, slot, test_support
from tools.dev.cell import CellError, ROOT, Settings, library_path
from tools.dev.cell import main as cell_main


def snapshot():
    return dict(available_mib=12000, total_mib=16384, cpus=4, cargo_cpus=4, memory_limit_mib=None, notes=[])


class SettingsTests(unittest.TestCase):
    def test_cells_are_explicit_and_preserve_overrides(self):
        for cell, feature, extension in [('bdb', 'bdb', 'db'), ('kc', 'kyotocabinet', 'kct'),
                                          ('tkrzw', 'tkrzw', 'tkt')]:
            settings = Settings.read({'PINYIN_ORACLE_DBM': cell, 'CARGO_TARGET_DIR': 'custom',
                                      'OXPINYIN_CAPI_SO': 'built/subject.so'})
            self.assertEqual((settings.cell, settings.feature, settings.extension),
                             (cell, feature, extension))
            self.assertEqual(settings.target, ROOT / 'custom')
            self.assertEqual(settings.environment['OXPINYIN_CAPI_SO'], str(ROOT / 'built/subject.so'))
            self.assertTrue(settings.expected_pin.endswith('+dbm-' + cell))

    def test_unknown_cell_is_status_three(self):
        with self.assertRaises(CellError) as error:
            Settings.read({'PINYIN_ORACLE_DBM': 'typo'})
        self.assertEqual(error.exception.status, 3)

    def test_claim_is_atomic_and_rejects_another_owner(self):
        with tempfile.TemporaryDirectory() as scratch:
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            settings.claim()
            settings.claim()
            path = Path(scratch) / 'oxpinyin-cell-identity.json'
            owner = json.loads(path.read_text())
            self.assertEqual(owner['artifacts'], {})
            other = Settings.read({'CARGO_TARGET_DIR': scratch, 'PINYIN_ORACLE_DBM': 'kc'})
            with self.assertRaises(CellError) as error:
                other.claim()
            self.assertEqual(error.exception.status, 3)
            self.assertEqual(json.loads(path.read_text()), owner)
            self.assertEqual(list(Path(scratch).iterdir()), [path])

    def test_unreadable_claim_fails_closed(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'oxpinyin-cell-identity.json'
            path.write_text('[]')
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            with self.assertRaisesRegex(CellError, 'unreadable identity record'):
                settings.claim()
            self.assertEqual(path.read_text(), '[]')

    def test_empty_selection_does_not_claim(self):
        with tempfile.TemporaryDirectory() as scratch:
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            self.assertEqual(settings.build([]), 0)
            self.assertEqual(list(Path(scratch).iterdir()), [])

    def test_loader_entries_keep_tokens_and_caller_spelling(self):
        cwd = Path('/caller root')
        self.assertEqual(library_path('./lib::/absolute:lib:', cwd),
                         '/caller root/./lib:/caller root:/absolute:/caller root/lib:/caller root')
        for token in ('$ORIGIN', '${ORIGIN}', '$LIB', '${LIB}', '$PLATFORM', '${PLATFORM}'):
            value = 'prefix/' + token + '/lib'
            self.assertEqual(library_path(value, cwd), value)
        self.assertEqual(library_path('', cwd), '')


class HeavyLaunchTests(unittest.TestCase):
    """Builds go through slot.run_command: admission taken here or inherited, cargo launched, never just."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.locks, self.target = Path(scratch.name) / 'locks', Path(scratch.name) / 'target'
        self.locks.mkdir()
        env = dict(os.environ, OXPINYIN_LOCK_DIR=str(self.locks), OXPINYIN_HEAVY_SLOTS='1',
                   OXPINYIN_RESOURCE_WAIT_SECONDS='5', CARGO_TARGET_DIR=str(self.target))
        for name in (slot.HEAVY_SLOT_HELD, 'CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'PINYIN_ORACLE_DBM',
                     'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO'):
            env.pop(name, None)
        self.env, self.out, self.calls = env, io.StringIO(), []
        self.enterContext(mock.patch.dict(os.environ, env, clear=True))
        self.enterContext(mock.patch.object(host, 'resource_snapshot', side_effect=snapshot))
        self.enterContext(contextlib.redirect_stderr(self.out))

    def recording(self):
        """Record the launches run_command makes, starting nothing."""
        return mock.patch.object(slot.subprocess, 'Popen', side_effect=self.popen)

    def popen(self, argv, *, env, pass_fds, start_new_session, cwd=None, stdout=None):
        """Record what run_command gives cargo; a test build prints one executable."""
        marker = json.loads(env[slot.HEAVY_SLOT_HELD])
        record = self.locks / f'{slot.HEAVY_SLOT}-{marker["slot"]}'
        self.calls.append(dict(argv=argv, cwd=cwd, stdout='file' if hasattr(stdout, 'write') else stdout,
                               session=start_new_session, marker=marker, fds=pass_fds, record=record,
                               holder=json.loads(record.read_text()),
                               descriptor=bool(pass_fds) and os.path.samestat(os.fstat(pass_fds[0]), record.stat()),
                               target=env.get('CARGO_TARGET_DIR'), jobs=env.get('CARGO_BUILD_JOBS')))
        if argv[1] == 'test':
            stdout.write(json.dumps(self.row()) + '\n')
            stdout.flush()
        process = mock.MagicMock(args=argv)
        process.__enter__.return_value = process
        process.wait.return_value = 0
        return process

    def row(self):
        return dict(reason='compiler-artifact', target=dict(name='user_dir_round_trip'),
                    executable=str(self.target / 'debug/deps/user_dir_round_trip-0'))

    def assert_admitted_here(self, call):
        """Cargo itself ran under this process's own slot, which was released afterwards."""
        self.assertEqual(call['argv'][0], 'cargo')
        self.assertNotIn('just', call['argv'])
        self.assertEqual((call['marker']['pid'], call['holder']['pid']), (os.getpid(), os.getpid()))
        self.assertEqual(call['holder']['command'], ' '.join(call['argv']))
        self.assertEqual(call['fds'], (call['marker']['fd'],))
        self.assertTrue(call['descriptor'])
        # run_command supervises cargo as the leader of its own process group.
        self.assertTrue(call['session'])
        self.assertEqual(call['target'], str(self.target))
        # Four CPUs with room for four rustc: the admitted default reaches cargo.
        self.assertEqual(call['jobs'], '4')
        self.assertEqual(call['record'].read_text(), '')

    def test_artifact_build_runs_cargo_through_run_command(self):
        library = self.target / 'debug/libpinyin_capi.so'
        library.parent.mkdir(parents=True)
        library.write_bytes(b'')
        settings = Settings.read(self.env)
        with self.recording():
            self.assertEqual(settings.artifact('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi'), library)
        [call] = self.calls
        self.assertEqual(call['argv'], ['cargo', 'build', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'),
                                        '-p', 'oxpinyin-capi', '--no-default-features', '--features', 'tkrzw'])
        self.assertEqual((call['cwd'], call['stdout']), (ROOT, 2))
        self.assert_admitted_here(call)
        self.assertIn('resources (admitted)', self.out.getvalue())

    def test_cell_build_runs_each_cargo_command_through_run_command(self):
        (self.target / 'debug').mkdir(parents=True)
        settings = Settings.read(self.env)
        with self.recording(), mock.patch.object(Settings, 'record') as record:
            self.assertEqual(settings.build(['pinyin-capi', 'zhuyin-capi', 'user-rt-test']), 0)
        self.assertEqual(record.call_count, 1)
        common = ['--locked', '--manifest-path', str(ROOT / 'Cargo.toml')]
        features = ['--no-default-features', '--features', 'tkrzw']
        self.assertEqual([call['argv'] for call in self.calls], [
            ['cargo', 'build', *common, '-p', 'oxpinyin-capi', '-p', 'oxpinyin-zhuyin-capi', *features],
            ['cargo', 'test', *common, '--profile', 'dev', '--no-run', '--message-format=json',
             '-p', 'oxpinyin-runtime', *features, '--test', 'user_dir_round_trip']])
        # The test build's messages go to a file run_command does not have to drain.
        self.assertEqual([(call['cwd'], call['stdout']) for call in self.calls], [(ROOT, None), (ROOT, 'file')])
        for call in self.calls:
            self.assert_admitted_here(call)
        self.assertEqual(os.readlink(self.target / 'debug/user-dir-round-trip-test'), self.row()['executable'])

    def test_table_utilities_share_one_package_build(self):
        settings = Settings.read(self.env)
        with self.recording(), mock.patch.object(Settings, 'record'):
            self.assertEqual(settings.build(['gen-pinyin-table', 'gen-zhuyin-table']), 0)
        self.assertEqual(len(self.calls), 1)
        self.assertEqual(self.calls[0]['argv'].count('oxpinyin-utils'), 1)
        self.assert_admitted_here(self.calls[0])

    def test_windows_builds_take_the_job_object_path_without_pass_fds(self):
        library = self.target / 'debug/libpinyin_capi.so'
        library.parent.mkdir(parents=True)
        library.write_bytes(b'')
        settings = Settings.read(self.env)
        with test_support.windows_launch(write=lambda stdout: stdout.write(json.dumps(self.row()) + '\n')) \
                as (launch, jobs, ntdll), mock.patch.object(Settings, 'record'):
            self.assertEqual(settings.artifact('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi'), library)
            self.assertEqual(settings.build(['pinyin-capi', 'user-rt-test']), 0)
        test_support.assert_windows_launches(self, launch, jobs, ntdll, cwd=ROOT)
        self.assertEqual([call.args[0][:2] for call in launch.call_args_list],
                         [['cargo', 'build'], ['cargo', 'build'], ['cargo', 'test']])
        self.assertEqual(launch.call_args_list[0].kwargs['stdout'], 2)
        self.assertEqual(os.readlink(self.target / 'debug/user-dir-round-trip-test'), self.row()['executable'])

    def test_a_cancelled_build_exits_with_128_plus_the_signal(self):
        with mock.patch.object(Settings, 'build', side_effect=slot.Cancelled('cancelled')), \
                mock.patch.object(slot, 'CANCELLED_SIGNAL', signal.SIGTERM):
            self.assertEqual(cell_main(['--artifacts', 'pinyin-capi'], environment=self.env), 128 + signal.SIGTERM)

    def test_a_build_under_a_holding_ancestor_inherits_its_slot(self):
        script = '''
import json, os, sys
from unittest import mock
from tools.dev import host, slot
from tools.dev.cell import Settings
seen = []
def popen(argv, *, env, pass_fds, **options):
    os.fstat(pass_fds[0])
    seen.append(dict(argv=argv, marker=json.loads(env['OXPINYIN_HEAVY_SLOT_HELD']), fds=list(pass_fds)))
    process = mock.MagicMock(args=argv)
    process.__enter__.return_value = process
    process.wait.return_value = 0
    return process
with mock.patch.object(host, 'resource_snapshot', side_effect=lambda: json.loads(sys.argv[1])), \\
        mock.patch.object(slot.subprocess, 'Popen', side_effect=popen), \\
        mock.patch.object(Settings, 'record'):
    status = Settings.read(os.environ).build(['pinyin-capi'])
print(json.dumps(dict(status=status, seen=seen)))
'''
        # One slot, held here: a child that failed to inherit it would wait out the limit and fail.
        with slot.HeavyAdmission('ancestor', self.env) as ancestor:
            fds = ancestor.slot.pass_fds()
            ancestor.slot.handoff()
            marker = json.loads(ancestor.env[slot.HEAVY_SLOT_HELD])
            result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', script,
                                     json.dumps(snapshot())], cwd=ROOT, env=ancestor.env, pass_fds=fds,
                                    capture_output=True, text=True, timeout=30)
            holder = json.loads((self.locks / f'{slot.HEAVY_SLOT}-{marker["slot"]}').read_text())
        self.assertEqual(result.returncode, 0, result.stderr)
        child = json.loads(result.stdout)
        self.assertEqual(child['status'], 0)
        [seen] = child['seen']
        self.assertEqual(seen['argv'][:2], ['cargo', 'build'])
        self.assertNotIn('just', seen['argv'])
        self.assertEqual((seen['marker']['pid'], seen['marker']['slot']), (os.getpid(), marker['slot']))
        self.assertEqual(seen['fds'], [marker['fd']])
        self.assertEqual(holder['pid'], os.getpid())
        self.assertIn(f'heavy slot {marker["slot"]} inherited from ancestor pid {os.getpid()}', result.stderr)

if __name__ == '__main__':
    unittest.main()
