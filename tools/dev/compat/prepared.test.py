#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Exercise prepared input wiring and resumption without compiling upstream code."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import doctor, registry
from tools.dev.cell import Settings
from tools.dev.frontend import FRONTEND_OBJECTS
from tools.dev.compat import prepared


class PreparedTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        self.prefix = self.work / 'oracle/bdb'
        data = self.prefix / 'lib/libpinyin/data'
        data.mkdir(parents=True)
        (data / 'table.conf').write_bytes(b'verified pin table')
        (self.prefix / 'lib/libpinyin.so').write_bytes(b'library')
        self.model = self.work / 'model'
        self.model.mkdir()
        (self.model / 'interpolation2.text').write_bytes(b'verified model')
        self.env = dict(PINYIN_ORACLE_DBM='bdb', PINYIN_MODEL_DIR=str(self.model),
                        OXPINYIN_COMPAT_IMAGE_ID='a' * 16,
                        OXPINYIN_TARGET_ROOT=str(self.work / 'targets'))
        self.settings = Settings.read(self.env)
        (self.prefix / 'oracle-pin.txt').write_text(self.settings.expected_pin + '\n')
        self.rows = registry.load()

    def prepare(self, *names):
        rows = [row for row in self.rows if row['name'] in names]
        return prepared.prepare('bdb', rows, self.prefix, self.work / 'cache', self.env)

    def fake_build(self, command, **options):
        self.assertTrue(options['heavy'])
        work = Path(command[command.index('--work-dir') + 1])
        prefix = Path(command[command.index('--prefix') + 1])
        if '--libs-only' in command:
            (prefix / 'lib').mkdir(parents=True)
            (prefix / 'lib/libpinyin.so').write_bytes(b'patched')
            expected = self.settings.oracle(str(self.prefix), 'bigram-export-strjoinv').environment['EXPECTED_ORACLE_PIN_REF']
            (prefix / 'oracle-pin.txt').write_text(expected + '\n')
        else:
            source = work / 'src/ibus-libpinyin-pin'
            (source / 'src').mkdir(parents=True)
            for name in FRONTEND_OBJECTS:
                (source / 'src' / name).touch()
            (source / 'data').mkdir()
            (source / 'data/com.github.libpinyin.ibus-libpinyin.gschema.xml').touch()

    def test_ordinary_selection_does_not_build_frontend_or_variants(self):
        with patch.object(prepared, 'run') as run:
            self.assertEqual(self.prepare('same-data-dir-diff'), [])
            run.assert_not_called()

    def test_tool_builds_are_cached_only_when_every_binary_exists(self):
        def build(command, **options):
            self.assertTrue(options['heavy'])
            if command[0] == 'bash':
                self.assertIn('--libs-only', command)
                work = Path(command[command.index('--work-dir') + 1])
                for directory in ('storage', 'training', 'segment'):
                    (work / 'src/libpinyin-pin/utils' / directory).mkdir(parents=True, exist_ok=True)
            else:
                self.assertEqual(command[0], 'make')
                made.append(command[2:])
                for name in command[3:]:
                    (Path(command[2]) / name).touch()
        made = []
        with patch.object(prepared, 'run', side_effect=build) as run:
            self.assertEqual(self.prepare('tool-diff'), [])
            self.assertEqual(run.call_count, 4)
            source = Path(self.env['OXPINYIN_TOOL_PIN_BUILD'])
            self.assertEqual(made, [[source / 'utils/storage', 'gen_pinyin_table', 'gen_zhuyin_table'],
                                    [source / 'utils/training', 'k_mixture_model_to_interpolation'],
                                    [source / 'utils/segment', 'ngseg', 'spseg', 'mergeseq']])
            self.assertEqual(self.prepare('tool-diff'), [])
            self.assertEqual(run.call_count, 4)
            (source / 'utils/training/k_mixture_model_to_interpolation').unlink()
            self.prepare('tool-diff')
            self.assertEqual(run.call_count, 8)

    def test_frontend_full_recipe_and_cell_assignment_are_cached(self):
        with patch.object(prepared, 'run', side_effect=self.fake_build) as run:
            options = self.prepare('import-diff')
            self.assertEqual(options[0], '--ibus-build')
            self.assertTrue(options[1].startswith('bdb='))
            command = run.call_args.args[0]
            self.assertEqual(command[:2], ['bash', str(ROOT / 'tools/oracle/build-oracle.sh')])
            self.assertIn('--model-dir', command)
            self.assertNotIn('--libs-only', command)
            self.assertEqual(self.prepare('import-diff'), options)
            self.assertEqual(run.call_count, 1)

    def test_frontend_rebuilds_when_a_required_object_is_missing_but_other_objects_remain(self):
        with patch.object(prepared, 'run', side_effect=self.fake_build) as run:
            options = self.prepare('import-diff')
            work = Path(options[1].split('=', 1)[1])
            for missing in ('ibus_engine_libpinyin-PYLibPinyin.o', 'ibus_engine_libpinyin-PYPConfig.o'):
                with self.subTest(missing=missing):
                    source = prepared.frontend_source(work)
                    self.assertIsNotNone(source)
                    (source / 'src' / missing).unlink()
                    self.assertGreater(len(list((source / 'src').glob('*.o'))), 0)
                    self.assertIsNone(prepared.frontend_source(work))
                    builds = run.call_count
                    self.assertEqual(self.prepare('import-diff'), options)
                    self.assertEqual(run.call_count, builds + 1)
                    self.assertIsNotNone(prepared.frontend_source(work))

    def test_failed_frontend_is_retried_and_never_marked_complete(self):
        with patch.object(prepared, 'run', side_effect=subprocess.CalledProcessError(5, 'build')):
            with self.assertRaises(subprocess.CalledProcessError):
                self.prepare('import-diff')
        self.assertFalse(list(self.work.rglob('.complete')))
        with patch.object(prepared, 'run', side_effect=self.fake_build) as run:
            self.prepare('import-diff')
            self.assertEqual(run.call_count, 1)

    def test_data_adds_model_text_without_changing_pin_tables_or_manifests(self):
        for name in ('dict-surface-diff', 'key-surface-diff', 'phrase-surface-diff', 'zhuyin-diff'):
            options = self.prepare(name)
            self.assertEqual(options[0], '--data')
            data = Path(options[1].split('=', 1)[1])
            self.assertEqual((data / 'interpolation2.text').read_bytes(), b'verified model')
            self.assertEqual((data / 'table.conf').read_bytes(), b'verified pin table')
            (data / 'table.conf').write_bytes(b'runner mutation')
        self.assertFalse((self.prefix / 'lib/libpinyin/data/interpolation2.text').exists())
        self.assertEqual((self.prefix / 'lib/libpinyin/data/table.conf').read_bytes(), b'verified pin table')

    def test_both_registered_variant_runners_share_one_distinct_patched_prefix(self):
        with patch.object(prepared, 'run', side_effect=self.fake_build) as run:
            self.assertEqual(self.prepare('bigram-export-diff', 'residue-a-tail-diff'), [])
            self.assertEqual(run.call_count, 1)
            command = run.call_args.args[0]
            self.assertIn('--libs-only', command)
            self.assertIn('--apply-patches', command)
            patched = Path(str(self.prefix) + '-bigram-export-strjoinv')
            self.assertEqual((patched / 'lib/libpinyin/data/table.conf').read_bytes(), b'verified pin table')
            self.prepare('residue-a-tail-diff')
            self.assertEqual(run.call_count, 1)
            self.env['OXPINYIN_COMPAT_IMAGE_ID'] = 'b' * 16
            self.prepare('bigram-export-diff')
            self.assertEqual(run.call_count, 2)

    def test_metadata_prepares_both_facades_in_an_isolated_target_and_passes_required_env(self):
        with patch.object(prepared, 'require_metadata_tool') as require, patch.object(prepared, 'run') as run:
            self.assertEqual(self.prepare('pc-metadata'), [])
            require.assert_called_once_with(self.env)
            self.assertEqual(run.call_count, 2)
            for call, library in zip(run.call_args_list, ('libpinyin', 'libzhuyin')):
                command = call.args[0]
                self.assertIn(library, command)
                self.assertIn('--profile', command)
                self.assertIn('dev', command)
                self.assertTrue(call.kwargs['heavy'])
                self.assertIn('/metadata/target', call.kwargs['env']['CARGO_TARGET_DIR'])
        pin_root = Path(self.env['OXPINYIN_METADATA_PIN_ROOT'])
        self.assertEqual((pin_root / self.prefix.relative_to('/') / 'oracle-pin.txt').read_text(),
                         self.settings.expected_pin + '\n')
        self.assertEqual(self.env['OXPINYIN_METADATA_PREFIX'], str(self.prefix))

    def test_metadata_refuses_an_old_distro_tool_before_building(self):
        tool = next(tool for tool in doctor.TOOLS if tool.name == 'cargo-c')
        with patch.object(doctor.shutil, 'which', return_value='/usr/bin/cargo-cinstall'), \
                patch.object(doctor, '_run', return_value=subprocess.CompletedProcess(
                    [], 0, stdout='cargo-c 0.10.11+cargo-0.86.0\n', stderr='')), \
                patch.object(prepared, 'run') as run:
            with self.assertRaisesRegex(doctor.DoctorError, tool.minimum):
                self.prepare('pc-metadata')
            run.assert_not_called()

    def test_metadata_refuses_a_missing_tool_before_building(self):
        with patch.object(prepared.shutil, 'which', return_value=None), \
                patch.object(prepared, 'run') as run:
            with self.assertRaisesRegex(doctor.DoctorError, 'needs cargo-c'):
                self.prepare('pc-metadata')
            run.assert_not_called()

    def test_metadata_accepts_a_distro_tool_above_the_shared_floor(self):
        tool = next(tool for tool in doctor.TOOLS if tool.name == 'cargo-c')
        with patch.object(doctor.shutil, 'which', return_value='/usr/bin/cargo-cinstall'), \
                patch.object(doctor, '_run', return_value=subprocess.CompletedProcess(
                    [], 0, stdout=f'cargo-c {tool.minimum}+cargo-0.98.0\n', stderr='')):
            prepared.require_metadata_tool(self.env)


if __name__ == '__main__':
    unittest.main()
