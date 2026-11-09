#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""tools/oracle/pin_capture.py: a capture of a rejected prefix, another libpinyin or libzhuyin,
other data, or a utility build not tied to a verified prefix is refused before any observation;
a capture writes nothing outside its staging directory; a utility run stages its end, streams
and tree and writes nothing outside its private copy."""
import json
import os
from pathlib import Path
import platform
import re
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import model  # noqa: E402
from tools.oracle import image, pin_capture  # noqa: E402

PINS = ('tools/oracle/oracle-pin.txt', 'tools/oracle/libpinyin-data.json')
RECIPE = ('tools/oracle/Dockerfile.oracle', 'tools/oracle/build-oracle.sh', 'tools/oracle/version-manifest.json',
          'tools/oracle/versions.py', 'tools/lib/bash-floor.sh')
UTILITIES = sorted({(spec.cell, subdir, program) for spec in pin_capture.OBSERVATIONS.values()
                    if isinstance(spec, (pin_capture.Utility, pin_capture.DataSequence))
                    for subdir, program in spec.programs()})


def take(name, identity):
    """A made-up observation; the checks under test never look at it."""
    return {'calls': [], 'results': [], 'end': {'exit': 0}}


def tree(root):
    return {path.relative_to(root): path.read_bytes() if path.is_file() else None
            for path in sorted(Path(root).rglob('*'))}


class CaptureTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for name in (*PINS, *RECIPE):
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).write_bytes((ROOT / name).read_bytes())
        self.data = self.root / 'data'
        self.data.mkdir()
        (self.data / 'table.conf').write_text('the fixture system data\n')
        pin = self.root / PINS[1]
        pin.write_text(json.dumps(dict(json.loads(pin.read_text()),
                                       content_sha256=pin_capture.contract_data.data_identity(self.data))))
        self.pins = image.fields(self.root / PINS[0])
        self.prefix = self.root / 'prefix'
        self.record = self.make_prefix(self.prefix, 'tkrzw')
        self.cache = self.root / 'provision'
        for cell in sorted({cell for cell, _, _ in UTILITIES}):
            self.make_prefix(self.cache / 'oracle' / cell, cell)
            built = self.tree(cell) / 'src/.libs/libpinyin.so.15.0.0'
            built.parent.mkdir(parents=True)
            built.write_bytes((self.cache / 'oracle' / cell / 'lib/libpinyin.so').read_bytes())
            (self.tree(cell) / '.git').mkdir()
            (self.tree(cell) / '.git/HEAD').write_text(self.pins['libpinyin_commit'] + '\n')
        for cell, subdir, program in UTILITIES:
            path = self.tree(cell) / 'utils' / subdir / program
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'\x7fELF a built ' + program.encode() + b'\n')
            path.chmod(0o755)
        self.model = self.root / 'model20/extracted'
        self.model.mkdir(parents=True)
        for name in model.EXPECTED_MODEL_FILES:
            (self.model / name).write_text('')
        (self.model.parent / 'verified').write_text(f'sha256={model.MODEL20_SHA256}\n')
        self.staging = self.root / 'target/pin-observations'

    def make_prefix(self, prefix, cell):
        (prefix / 'lib').mkdir(parents=True)
        (prefix / 'lib/libpinyin.so').write_text(f'a {cell} libpinyin build\n')
        (prefix / 'lib/libzhuyin.so').write_text(f'a {cell} libzhuyin build\n')
        (prefix / 'versions.txt').write_text('installed versions\n')
        record = {
            'recipe_sha256': image.recipe_hash(self.root), 'architecture': platform.machine(),
            'versions_sha256': image.digest_file(prefix / 'versions.txt'),
            'libpinyin_commit': self.pins['libpinyin_commit'],
            'ibus_libpinyin_commit': self.pins['ibus_libpinyin_commit'],
            'dbm': image.cell.BACKEND_FORMATS[image.cell.CELL_TO_BACKEND[cell]],
            'shared_object_sha256': image.digest_file(prefix / 'lib/libpinyin.so'),
            'zhuyin_shared_object_sha256': image.digest_file(prefix / 'lib/libzhuyin.so'),
        }
        self.write_record(prefix, record)
        return record

    @staticmethod
    def write_record(prefix, record, **change):
        (prefix / 'oracle-pin.txt').write_text(''.join(f'{key}={value}\n' for key, value in dict(record, **change).items()))

    def tree(self, cell):
        return pin_capture.build_tree(self.cache, cell, self.pins['libpinyin_commit'])

    def capture(self, run='1', libraries=None, data=None, taken=take, cache=True, extraction=True):
        return pin_capture.capture(run, self.prefix, data or self.data, self.staging, taken, root=self.root,
                                   libraries=libraries, cache=self.cache if cache else None,
                                   model=self.model if extraction else None)

    def assert_refused(self, message, **kwargs):
        before = tree(self.root)
        taken = mock.Mock(side_effect=take)
        with self.assertRaisesRegex(pin_capture.CaptureError, message):
            self.capture(taken=taken, **kwargs)
        taken.assert_not_called()
        self.assertEqual(tree(self.root), before, 'a refused capture writes nothing')

    def test_a_prefix_that_prefix_ok_rejects_is_refused(self):
        for label, change in (
            ('source commit', {'libpinyin_commit': '0c5e80e1200f84fab185d1c5bde458b770a0636c'}),
            ('recipe', {'recipe_sha256': '0' * 64}),
            ('backend', {'dbm': 'BerkeleyDB'}),
            ('library record', {'shared_object_sha256': '0' * 64}),
            ('zhuyin library record', {'zhuyin_shared_object_sha256': '0' * 64}),
            ('versions record', {'versions_sha256': '0' * 64}),
        ):
            with self.subTest(label):
                self.write_record(self.prefix, self.record, **change)
                self.assert_refused('prefix_ok refused it')
        self.write_record(self.prefix, self.record)
        (self.prefix / 'oracle-pin.txt').unlink()
        self.assert_refused('prefix_ok refused it')

    def test_a_loaded_library_that_differs_from_the_prefix_record_is_refused(self):
        for facade in ('pinyin', 'zhuyin'):
            with self.subTest(facade):
                other = self.root / f'other-lib{facade}.so'
                other.write_text(f'another lib{facade} build\n')
                self.assert_refused(f'the loaded lib{facade} is', libraries={facade: other})

    def test_other_data_is_refused(self):
        other = self.root / 'other-data'
        other.mkdir()
        (other / 'table.conf').write_text('other system data\n')
        self.assert_refused('the data digest is', data=other)

    def test_the_utility_runs_need_a_provision_cache(self):
        self.assert_refused('need --provision-cache', cache=False)

    def test_the_data_sequence_needs_a_verified_model20_extraction(self):
        self.assert_refused('needs --model-dir', extraction=False)
        (self.model.parent / 'verified').unlink()
        self.assert_refused('does not record the pinned archive digest')
        (self.model.parent / 'verified').write_text('sha256=' + '0' * 64 + '\n')
        self.assert_refused('does not record the pinned archive digest')

    def test_a_utility_cell_prefix_that_prefix_ok_rejects_is_refused(self):
        prefix = self.cache / 'oracle/kc'
        record = image.fields(prefix / 'oracle-pin.txt')
        for label, change in (('backend', {'dbm': 'Tkrzw'}), ('library record', {'shared_object_sha256': '0' * 64})):
            with self.subTest(label):
                self.write_record(prefix, record, **change)
                # The message names the prefix as the host spells its path.
                self.assert_refused(re.escape(str(Path('oracle', 'kc'))) + ': prefix_ok refused it')
        self.write_record(prefix, record)

    def test_a_build_tree_of_another_commit_is_refused(self):
        head = self.tree('bdb') / '.git/HEAD'
        head.write_text('0c5e80e1200f84fab185d1c5bde458b770a0636c\n')
        self.assert_refused('is not the pin commit')
        head.unlink()
        self.assert_refused('is not the pin commit')

    def test_a_build_tree_whose_library_differs_from_the_prefix_record_is_refused(self):
        built = self.tree('tkrzw') / 'src/.libs/libpinyin.so.15.0.0'
        built.write_text('a library another recipe built\n')
        self.assert_refused('the build tree built')
        built.unlink()
        self.assert_refused('no built src/.libs/libpinyin.so')

    def test_a_utility_that_is_not_a_built_executable_is_refused(self):
        cell, subdir, program = UTILITIES[0]
        path = self.tree(cell) / 'utils' / subdir / program
        for label, setup in (('libtool wrapper', lambda: path.write_text('#! /bin/sh\n# libtool wrapper\n')),
                             ('missing', path.unlink)):
            with self.subTest(label):
                setup()
                self.assert_refused('not a built utility executable')

    if os.name == 'posix':
        # Windows has no execute bit: os.access(X_OK) is true for every file there.
        def test_a_utility_without_the_execute_bit_is_refused(self):
            cell, subdir, program = UTILITIES[0]
            path = self.tree(cell) / 'utils' / subdir / program
            path.write_bytes(b'\x7fELF')
            path.chmod(0o644)
            self.assert_refused('not a built utility executable')

    def test_nothing_is_written_outside_the_staging_directory(self):
        before = tree(self.root)
        with self.assertRaisesRegex(pin_capture.CaptureError, 'not a single path component'):
            self.capture(run='../outside')
        self.assertEqual(tree(self.root), before, 'a run name that leaves the staging directory writes nothing')
        with mock.patch.dict(os.environ, {'OXPINYIN_CAPTURE_IMAGE': 'sha256:capture'}):
            self.assertEqual(self.capture(), 0)
        after = tree(self.root)
        self.assertEqual({path: after[path] for path in before}, before, 'no existing path changes')
        staged = Path(self.staging.relative_to(self.root)) / '1'
        self.assertEqual(sorted(set(after) - set(before)),
                         [Path('target'), Path('target/pin-observations'), staged,
                          staged / 'capture.json', staged / 'observations.json'])
        identity = json.loads(after[staged / 'capture.json'])
        self.assertEqual({key: identity[key] for key in ('image', 'pin_commit', 'recipe_sha256', 'library_sha256',
                                                         'versions_sha256')},
                         {'image': 'sha256:capture', 'pin_commit': self.record['libpinyin_commit'],
                          'recipe_sha256': self.record['recipe_sha256'],
                          'library_sha256': self.record['shared_object_sha256'],
                          'versions_sha256': self.record['versions_sha256']})
        self.assertEqual(identity['facades']['zhuyin']['library_sha256'], self.record['zhuyin_shared_object_sha256'])
        cell, subdir, program = UTILITIES[0]
        self.assertEqual(identity['utility_cells'][cell]['utilities'][program]['sha256'],
                         image.digest_file(self.tree(cell) / 'utils' / subdir / program))
        self.assertEqual(identity['model_dir'], str(self.model.resolve()))
        self.assertEqual(sorted(json.loads(after[staged / 'observations.json'])), sorted(pin_capture.OBSERVATIONS))

    def test_a_staged_run_is_not_repeated(self):
        self.assertEqual(self.capture(), 0)
        self.assert_refused('stage each run once')


class UtilityRunTests(unittest.TestCase):
    if os.name == 'posix':
        # The utility is a '#!/bin/sh' script that Windows cannot launch.
        def test_a_run_stages_its_end_streams_and_tree_and_leaves_the_fixture_alone(self):
            with tempfile.TemporaryDirectory() as scratch:
                root = Path(scratch)
                fixture = root / 'fixtures/w3/tkt'
                fixture.mkdir(parents=True)
                (fixture / 'table.conf').write_bytes(b'conf\n')
                (fixture / 'kept.bin').write_bytes(b'kept\n')
                program = root / 'utility'
                program.write_text('#!/bin/sh\ncat > out.txt\nprintf more >> table.conf\nrm kept.bin\n'
                                   'printf out; printf err >&2; exit 3\n')
                program.chmod(0o755)
                spec = pin_capture.Utility('tkrzw', 'training', 'utility', (), b'\xff stdin\n')
                before = tree(root)
                with mock.patch.dict(pin_capture.OBSERVATIONS, {'utility/test': spec}):
                    observation = pin_capture.run_utility('utility/test', program, root)
                self.assertEqual(tree(root), before, 'the run changes only its private copy')
            self.assertEqual(observation['end'], {'exit': 3})
            self.assertEqual((observation['stdin'], observation['stdout'], observation['stderr']),
                             ({'hex': 'ff20737464696e0a'}, {'text': 'out'}, {'text': 'err'}))
            files = {entry['path']: entry for entry in observation['files']}
            self.assertEqual(files['kept.bin'], {'path': 'kept.bin', 'kind': 'removed'})
            self.assertEqual((files['table.conf']['unchanged'], files['table.conf']['size']), (False, 9))
            self.assertEqual((files['out.txt']['unchanged'], files['out.txt']['size'], files['out.txt']['mode'][:2]),
                             (False, 8, '0o'))
            self.assertNotIn('rows', files['out.txt'], 'only DBM files are read as rows')

    def test_runs_that_differ_only_in_a_tkrzw_clock_or_bdb_container_bytes_stage_equal_entries(self):
        tkrzw = b'TkrzwHDB\n' + bytes(39) + b'%s' + bytes(72)  # 128 bytes, the clock at 48-56
        cases = (('tkrzw', 'bigram.db', tkrzw % b'clock--1', tkrzw % b'clock--2'),
                 ('bdb', 'phrase_index.bin', b'container bytes, run 1', b'container bytes, run 2'),
                 ('kc', 'deleted.db', b'kc container, run 1', b'kc container, run 2'))
        rows_ = [['01000000', 'beef']]
        for cell, name, first, second in cases:
            with self.subTest(cell):
                staged = []
                for data in (first, second):
                    with tempfile.TemporaryDirectory() as scratch:
                        work = Path(scratch)
                        (work / name).write_bytes(data)
                        (work / 'out.txt').write_bytes(b'not a DBM file\n')
                        with mock.patch.object(pin_capture, 'dbm_rows', return_value=rows_):
                            staged.append(pin_capture.tree(work, {'out.txt': b'before\n'}, cell))
                self.assertEqual(staged[0], staged[1])
                entry = next(entry for entry in staged[0] if entry['path'] == name)
                self.assertNotIn('sha256', entry, 'no raw hash of a changed DBM file')
                self.assertEqual(entry['rows'], rows_)
                self.assertEqual(entry['rows_sha256'], pin_capture.rows_digest(rows_))
                self.assertEqual('sha256_clock_masked' in entry, cell == 'tkrzw')
                self.assertIn('sha256', next(entry for entry in staged[0] if entry['path'] == 'out.txt'))

    def test_rows_and_grams_follow_the_frozen_layout(self):
        self.assertEqual(pin_capture.rows_digest([]),
                         'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855')  # NO_ROWS
        value = b''.join(word.to_bytes(4, 'little') for word in (5, 16777220, 5))
        decoded = pin_capture.grams([[(16777219).to_bytes(4, 'little').hex(), value.hex()]], (1, 16777219))
        self.assertEqual(decoded, {'1': None, '16777219': {'total': 5, 'items': [[16777220, 5]]}})


if __name__ == '__main__':
    unittest.main()
