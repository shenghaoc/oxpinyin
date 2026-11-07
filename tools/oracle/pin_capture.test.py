#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""tools/oracle/pin_capture.py: a capture of a rejected prefix, another library or other data is
refused before any observation, and a capture writes nothing outside its staging directory."""
import json
import os
from pathlib import Path
import platform
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.oracle import image, pin_capture  # noqa: E402

PINS = ('tools/oracle/oracle-pin.txt', 'tools/oracle/libpinyin-data.json')
RECIPE = ('tools/oracle/Dockerfile.oracle', 'tools/oracle/build-oracle.sh', 'tools/oracle/version-manifest.json',
          'tools/oracle/versions.py', 'tools/tkrzw/distro-probe.sh', 'tools/lib/bash-floor.sh')


def take(name):
    """A made-up observation; the check under test never looks at it."""
    return {'input': pin_capture.OBSERVATIONS[name][1], 'calls': [], 'results': [], 'end': {'exit': 0}}


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
                                       content_sha256=pin_capture.reference.data_identity(self.data))))
        self.prefix = self.root / 'prefix'
        (self.prefix / 'lib').mkdir(parents=True)
        self.library = self.prefix / 'lib/libpinyin.so'
        self.library.write_text('a libpinyin build\n')
        (self.prefix / 'lib/libzhuyin.so').write_text('a libzhuyin build\n')
        (self.prefix / 'versions.txt').write_text('installed versions\n')
        pins = image.fields(self.root / PINS[0])
        self.record = {
            'recipe_sha256': image.recipe_hash(self.root), 'architecture': platform.machine(),
            'versions_sha256': image.digest_file(self.prefix / 'versions.txt'),
            'libpinyin_commit': pins['libpinyin_commit'], 'ibus_libpinyin_commit': pins['ibus_libpinyin_commit'],
            'dbm': image.cell.BACKEND_FORMATS[image.cell.CELL_TO_BACKEND['tkrzw']],
            'shared_object_sha256': image.digest_file(self.library),
            'zhuyin_shared_object_sha256': image.digest_file(self.prefix / 'lib/libzhuyin.so'),
        }
        self.write_record()
        self.staging = self.root / 'target/pin-observations'

    def write_record(self, **change):
        (self.prefix / 'oracle-pin.txt').write_text(
            ''.join(f'{key}={value}\n' for key, value in dict(self.record, **change).items()))

    def capture(self, run='1', library=None, data=None, taken=take):
        return pin_capture.capture(run, self.prefix, library or self.library, data or self.data,
                                   self.staging, taken, root=self.root)

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
            ('versions record', {'versions_sha256': '0' * 64}),
        ):
            with self.subTest(label):
                self.write_record(**change)
                self.assert_refused('prefix_ok refused it')
        self.write_record()
        (self.prefix / 'oracle-pin.txt').unlink()
        self.assert_refused('prefix_ok refused it')

    def test_a_loaded_library_that_differs_from_the_prefix_record_is_refused(self):
        other = self.root / 'other-libpinyin.so'
        other.write_text('another libpinyin build\n')
        self.assert_refused('the loaded library is', library=other)

    def test_other_data_is_refused(self):
        other = self.root / 'other-data'
        other.mkdir()
        (other / 'table.conf').write_text('other system data\n')
        self.assert_refused('the data digest is', data=other)

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
        self.assertEqual(sorted(json.loads(after[staged / 'observations.json'])), sorted(pin_capture.OBSERVATIONS))

    def test_a_staged_run_is_not_repeated(self):
        self.assertEqual(self.capture(), 0)
        self.assert_refused('stage each run once')


if __name__ == '__main__':
    unittest.main()
