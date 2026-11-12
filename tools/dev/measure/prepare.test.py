#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Tests for the Valgrind recipes' input preparation: prepared pin prefixes are reused only when
every recorded input still matches, and are rebuilt otherwise."""
import contextlib
import io
import shutil
from pathlib import Path
import platform
import re
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import runners
from tools.dev.measure import bench, prepare
from tools.oracle import image, versions


class PreparationTests(unittest.TestCase):
    def prepared_prefix(self, prefix):
        """A prepared PREFIX that passes every reuse check; answer its record's text."""
        import hashlib
        data = prefix / 'lib/libpinyin/data'
        data.mkdir(parents=True)
        pins = image.fields(ROOT / 'tools/oracle/oracle-pin.txt')
        installed = mock.patch.object(versions, 'package_version', return_value='1.0')
        installed.start()
        self.addCleanup(installed.stop)
        (prefix / 'versions.txt').write_text(prepare.running_inventory())
        fields = dict(libpinyin_commit=pins['libpinyin_commit'], ibus_libpinyin_commit=pins['ibus_libpinyin_commit'],
                      model_sha256=pins['model_archive_sha256'],
                      recipe_sha256=image.recipe_hash(), architecture=platform.machine(),
                      dbm=image.cell.BACKEND_FORMATS[image.cell.CELL_TO_BACKEND['tkrzw']],
                      versions_sha256=image.digest_file(prefix / 'versions.txt'))
        for library, key, content in (('libpinyin.so', 'shared_object_sha256', b'pinyin'),
                                      ('libzhuyin.so', 'zhuyin_shared_object_sha256', b'zhuyin')):
            (prefix / 'lib' / library).write_bytes(content)
            fields[key] = hashlib.sha256(content).hexdigest()
        text = ''.join(f'{key}={value}\n' for key, value in fields.items())
        for manifest, key, file in (('oracle-data.sha256', 'data_manifest_sha256', 'table.conf'),
                                    ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256', 'bigram.db')):
            (data / file).write_bytes(b'data')
            content = f'{hashlib.sha256(b"data").hexdigest()}  lib/libpinyin/data/{file}\n'.encode()
            (prefix / manifest).write_bytes(content)
            text += f'{key}={hashlib.sha256(content).hexdigest()}\n'
        (prefix / 'oracle-pin.txt').write_text(text)
        return text

    def test_prepared_data_reuse_checks_prefix_ok_and_file_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory) / 'prepared'
            text = self.prepared_prefix(prefix)
            data = prefix / 'lib/libpinyin/data'
            self.assertTrue(prepare.prepared_pin_matches(prefix, 'tkrzw'))
            (data / 'bigram.db').write_bytes(b'damaged')
            self.assertFalse(prepare.prepared_pin_matches(prefix, 'tkrzw'))
            (data / 'bigram.db').write_bytes(b'data')
            (prefix / 'oracle-pin.txt').write_text(re.sub(r'recipe_sha256=\w+', 'recipe_sha256=other', text))
            self.assertFalse(prepare.prepared_pin_matches(prefix, 'tkrzw'))

    def prepare_oracle(self, root):
        """Run `prepare oracle --cells tkrzw` with /opt and /performance under ROOT; answer the regeneration mock."""
        from tools.dev import model
        from tools.dev.measure import recipes
        def path(value):
            value = Path(value)
            return root / value.relative_to('/') if value.anchor and value.parts[1] in ('opt', 'performance') else value
        with mock.patch.object(prepare, 'Path', side_effect=path), \
             mock.patch.object(recipes, 'require_performance_environment'), \
             mock.patch.object(model, 'fetch', return_value=root / 'model'), \
             mock.patch.object(bench, 'heavy') as regenerate:
            self.assertEqual(prepare.prepare(['oracle', '--cells', 'tkrzw'], {}), 0)
        return regenerate

    def prepared_and_seed(self, directory):
        """A valid prepared prefix and an identical seed under DIRECTORY; answer both and the record's text."""
        prefix, seed = Path(directory) / 'performance/pinyin-oracle/tkrzw', Path(directory) / 'opt/pinyin-oracle/tkrzw'
        text = self.prepared_prefix(prefix)
        shutil.copytree(prefix, seed)
        return prefix, seed, text

    def test_a_cached_library_with_changed_bytes_is_refused_and_rebuilt(self):
        for library, content in (('libpinyin.so', b'pinyin'), ('libzhuyin.so', b'zhuyin')):
            with self.subTest(library=library), tempfile.TemporaryDirectory() as directory:
                prefix, _, _ = self.prepared_and_seed(directory)
                record = (prefix / 'oracle-pin.txt').read_bytes()
                (prefix / 'lib' / library).write_bytes(b'changed')
                self.assertEqual((prefix / 'oracle-pin.txt').read_bytes(), record)
                self.assertFalse(prepare.prepared_pin_matches(prefix, 'tkrzw'))
                self.assertIn('--regen-data', self.prepare_oracle(Path(directory)).call_args.args[0])
                self.assertEqual((prefix / 'lib' / library).read_bytes(), content)

    def test_a_prefix_with_another_images_versions_is_refused_and_rebuilt(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix, seed, text = self.prepared_and_seed(directory)
            self.assertTrue(prepare.prepared_pin_matches(prefix, 'tkrzw'))
            with mock.patch.object(versions, 'package_version', return_value='2.0'):
                current = prepare.running_inventory()
                (seed / 'versions.txt').write_text(current)
                (seed / 'oracle-pin.txt').write_text(re.sub(
                    r'versions_sha256=\w+', 'versions_sha256=' + image.digest_file(seed / 'versions.txt'), text))
                self.assertTrue(image.prefix_ok(prefix, 'tkrzw'), 'the recorded inventory is intact')
                self.assertFalse(prepare.prepared_pin_matches(prefix, 'tkrzw'))
                self.assertIn('--regen-data', self.prepare_oracle(Path(directory)).call_args.args[0])
            self.assertEqual((prefix / 'versions.txt').read_text(), current)

    def test_a_prefix_without_the_pinned_model_is_refused_and_rebuilt(self):
        for label, change in (('missing', lambda text: re.sub(r'model_sha256=\w+\n', '', text)),
                              ('different', lambda text: re.sub(r'model_sha256=\w+', 'model_sha256=' + '0' * 64, text))):
            with self.subTest(label), tempfile.TemporaryDirectory() as directory:
                prefix, _, text = self.prepared_and_seed(directory)
                (prefix / 'oracle-pin.txt').write_text(change(text))
                self.assertTrue(image.prefix_ok(prefix, 'tkrzw'), 'prefix_ok does not check the model')
                self.assertFalse(prepare.prepared_pin_matches(prefix, 'tkrzw'))
                self.assertIn('--regen-data', self.prepare_oracle(Path(directory)).call_args.args[0])
                self.assertEqual((prefix / 'oracle-pin.txt').read_text(), text)

    def test_a_rebuild_that_still_does_not_match_fails_preparation(self):
        def another_model(seed, text):
            (seed / 'oracle-pin.txt').write_text(re.sub(r'model_sha256=\w+', 'model_sha256=' + '0' * 64, text))

        for label, damage, installed, named in (
            ('model', another_model, '1.0', 'model_sha256'),
            ('library', lambda seed, text: (seed / 'lib/libpinyin.so').write_bytes(b'changed'), '1.0', 'prefix_ok'),
            ('data', lambda seed, text: (seed / 'lib/libpinyin/data/bigram.db').write_bytes(b'changed'), '1.0', 'bigram.db'),
            ('inventory', lambda seed, text: None, '2.0', "the running image's inventory"),
        ):
            with self.subTest(label), tempfile.TemporaryDirectory() as directory:
                prefix, seed, text = self.prepared_and_seed(directory)
                shutil.rmtree(prefix)
                damage(seed, text)
                with mock.patch.object(versions, 'package_version', return_value=installed), \
                     contextlib.redirect_stderr(io.StringIO()) as stderr, self.assertRaises(runners.Finished):
                    self.prepare_oracle(Path(directory))
                self.assertIn(named, stderr.getvalue())
                self.assertFalse((Path(directory) / 'opt/libpinyin-tkrzw').exists(),
                                 'a refused prefix is not reported as prepared')

    def test_cells_is_only_accepted_for_oracle_preparation(self):
        from tools.dev import model
        from tools.dev.measure import recipes
        for mode in ('callgrind',):
            with self.subTest(mode=mode), mock.patch.object(recipes, 'require_performance_environment'), \
                 mock.patch.object(model, 'fetch') as fetch, contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    prepare.prepare([mode, '--cells', 'tkrzw'], {})
                self.assertEqual(error.exception.code, 2)
                fetch.assert_not_called()

    def test_preparation_refuses_outside_the_stage_before_fetching_model_data(self):
        from tools.dev import model
        with mock.patch.object(model, 'fetch') as fetch, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(runners.Finished):
                prepare.prepare(['oracle'], {'GITHUB_ACTIONS': 'true'})
            fetch.assert_not_called()


if __name__ == '__main__':
    unittest.main()
