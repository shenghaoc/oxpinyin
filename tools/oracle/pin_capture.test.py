#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""tools/oracle/pin_capture.py: a capture of another pin or other data is refused, and a capture
writes nothing outside its staging directory."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.oracle import image, pin_capture  # noqa: E402

PINS = ('tools/oracle/oracle-pin.txt', 'tools/oracle/libpinyin-data.json')


def pinned_identity():
    """capture.json of a capture that matches this repository's pins, on linux/arm64."""
    pins = ROOT / PINS[0]
    data = json.loads((ROOT / PINS[1]).read_text())
    return {'platform': 'linux/arm64', 'pin_commit': image.fields(pins)['libpinyin_commit'],
            'library_sha256': image.platform_field(pins, 'linux/arm64', 'libpinyin_sha256_tkrzw'),
            'data': {'kind': 'package', 'digest': data['content_sha256']}}


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
        for name in PINS:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).write_bytes((ROOT / name).read_bytes())
        (self.root / 'fixtures').mkdir()
        self.staging = self.root / 'target/pin-observations'

    def capture(self, identity, run='1'):
        return pin_capture.capture(run, identity, self.staging, take, root=self.root)

    def test_a_capture_of_another_pin_or_other_data_is_refused(self):
        for label, change in (
            ('pin commit', {'pin_commit': '0c5e80e1200f84fab185d1c5bde458b770a0636c'}),
            ('library SHA-256', {'library_sha256': '0' * 64}),
            ('library SHA-256', {'platform': 'linux/riscv64'}),
            ('data identity', {'data': {'kind': 'package', 'digest': '0' * 64}}),
            ('data identity', {'data': {'kind': 'w3', 'digest': pinned_identity()['data']['digest']}}),
        ):
            with self.subTest(change=change):
                before = tree(self.root)
                with self.assertRaisesRegex(pin_capture.CaptureError, f'the {label} is'):
                    self.capture(dict(pinned_identity(), **change))
                self.assertEqual(tree(self.root), before, 'a refused capture writes nothing')

    def test_nothing_is_written_outside_the_staging_directory(self):
        before = tree(self.root)
        with self.assertRaisesRegex(pin_capture.CaptureError, 'not a single path component'):
            self.capture(pinned_identity(), run='../outside')
        self.assertEqual(tree(self.root), before, 'a run name that leaves the staging directory writes nothing')
        self.assertEqual(self.capture(pinned_identity()), 0)
        after = tree(self.root)
        self.assertEqual({path: after[path] for path in before}, before, 'no existing path changes')
        staged = Path(self.staging.relative_to(self.root)) / '1'
        self.assertEqual(sorted(set(after) - set(before)),
                         [Path('target'), Path('target/pin-observations'), staged,
                          staged / 'capture.json', staged / 'observations.json'])
        self.assertEqual(json.loads(after[staged / 'capture.json']), pinned_identity())
        self.assertEqual(sorted(json.loads(after[staged / 'observations.json'])), sorted(pin_capture.OBSERVATIONS))


if __name__ == '__main__':
    unittest.main()
