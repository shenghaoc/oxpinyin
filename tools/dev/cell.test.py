#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Portable cell settings, loader paths, atomic ownership and build launch tests."""
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import tempfile
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from tools.dev import cell as backend_cell, slot, test_support
from tools.dev.cell import CellError, ROOT, Settings, library_path
from tools.dev.cell import main as cell_main


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

    def test_identity_record_names_the_cdylibs_of_this_host(self):
        expected = {'win32': ('pinyin_capi.dll', 'zhuyin_capi.dll'),
                    'darwin': ('libpinyin_capi.dylib', 'libzhuyin_capi.dylib')}.get(
                        sys.platform, ('libpinyin_capi.so', 'libzhuyin_capi.so'))
        with tempfile.TemporaryDirectory() as scratch:
            settings = Settings.read({**os.environ, 'CARGO_TARGET_DIR': scratch})
            settings.claim()
            for filename in expected:
                (Path(scratch) / 'debug').mkdir(exist_ok=True)
                (Path(scratch) / 'debug' / filename).write_bytes(b'0123')
            settings.record()
            artifacts = json.loads((Path(scratch) / 'oxpinyin-cell-identity.json').read_text())['artifacts']
            self.assertEqual(sorted(artifacts), ['pinyin-capi', 'zhuyin-capi'])
            for name, filename in zip(('pinyin-capi', 'zhuyin-capi'), expected):
                self.assertEqual(artifacts[name]['path'], str(Path(scratch) / 'debug' / filename))
                self.assertEqual(artifacts[name]['size'], 4)

    def test_unreadable_claim_fails_closed(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'oxpinyin-cell-identity.json'
            path.write_text('[]')
            settings = Settings.read({'CARGO_TARGET_DIR': scratch})
            with self.assertRaisesRegex(CellError, 'unreadable identity record'):
                settings.claim()
            self.assertEqual(path.read_text(), '[]')

    def test_loader_entries_keep_tokens_and_caller_spelling(self):
        cwd = Path('/caller root')
        self.assertEqual(library_path('./lib::/absolute:lib:', cwd),
                         '/caller root/./lib:/caller root:/absolute:/caller root/lib:/caller root')
        for token in ('$ORIGIN', '${ORIGIN}', '$LIB', '${LIB}', '$PLATFORM', '${PLATFORM}'):
            value = 'prefix/' + token + '/lib'
            self.assertEqual(library_path(value, cwd), value)
        self.assertEqual(library_path('', cwd), '')


class BuildLaunchTests(unittest.TestCase):
    """Builds go through slot.run_command: cargo launched directly, never just."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.target = Path(scratch.name) / 'target'
        env = dict(os.environ, CARGO_TARGET_DIR=str(self.target))
        for name in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'PINYIN_ORACLE_DBM',
                     'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO'):
            env.pop(name, None)
        self.env, self.out, self.calls = env, io.StringIO(), []
        backend_cell.declared_default()
        self.enterContext(mock.patch.dict(os.environ, env, clear=True))
        self.enterContext(contextlib.redirect_stderr(self.out))

    def recording(self):
        """Record the launches run_command makes, starting nothing."""
        return mock.patch.object(slot.subprocess, 'Popen', side_effect=self.popen)

    def popen(self, argv, *, env, start_new_session, cwd=None, stdout=None):
        """Record what run_command gives cargo."""
        self.calls.append(dict(argv=argv, cwd=cwd, stdout='file' if hasattr(stdout, 'write') else stdout,
                               session=start_new_session, target=env.get('CARGO_TARGET_DIR'),
                               jobs=env.get('CARGO_BUILD_JOBS')))
        process = mock.MagicMock(args=argv)
        process.__enter__.return_value = process
        process.wait.return_value = 0
        return process

    def assert_supervised(self, call):
        """Cargo itself ran, supervised as the leader of its own process group."""
        self.assertEqual(call['argv'][0], 'cargo')
        self.assertNotIn('just', call['argv'])
        self.assertTrue(call['session'])
        self.assertEqual(call['target'], str(self.target))
        # No job count is chosen here: cargo's own default applies.
        self.assertIsNone(call['jobs'])

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
        self.assert_supervised(call)

    def test_windows_builds_take_the_job_object_path(self):
        library = self.target / 'debug/libpinyin_capi.so'
        library.parent.mkdir(parents=True)
        library.write_bytes(b'')
        settings = Settings.read(self.env)
        with test_support.windows_launch() as (launch, jobs, ntdll), mock.patch.object(Settings, 'record'):
            self.assertEqual(settings.artifact('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi'), library)
            self.assertEqual(settings.build(['pinyin-capi']), 0)
        test_support.assert_windows_launches(self, launch, jobs, ntdll, cwd=ROOT)
        self.assertEqual([call.args[0][:2] for call in launch.call_args_list],
                         [['cargo', 'build'], ['cargo', 'build']])
        self.assertEqual(launch.call_args_list[0].kwargs['stdout'], 2)

    def test_a_cancelled_build_exits_with_128_plus_the_signal(self):
        with mock.patch.object(Settings, 'build', side_effect=slot.Cancelled('cancelled')), \
                mock.patch.object(slot, 'CANCELLED_SIGNAL', signal.SIGTERM):
            self.assertEqual(cell_main(['--artifacts', 'pinyin-capi'], environment=self.env), 128 + signal.SIGTERM)


if __name__ == '__main__':
    unittest.main()
