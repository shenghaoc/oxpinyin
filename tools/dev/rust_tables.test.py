"""Pinned scripts2 sources, offline regeneration, value identity and drift rejection."""
import contextlib
import io
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import rust_tables


class RustTablesTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.outputs = rust_tables.render()

    def test_committed_tables_regenerate_offline_without_any_external_program(self):
        # In particular, no rustfmt/rustup process can download a toolchain in
        # the Python-only CI harness, and no source/input download is hidden.
        with mock.patch('subprocess.run', side_effect=AssertionError('external command')):
            with mock.patch('subprocess.check_output', side_effect=AssertionError('external command')):
                with mock.patch('socket.socket', side_effect=AssertionError('network')):
                    self.assertEqual(rust_tables.regenerate(check=True), 0)

    def test_changed_generated_value_is_rejected_and_check_does_not_rewrite_it(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name, content in self.outputs.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding='utf-8', newline='\n')
            path = root / 'crates/oxpinyin-core/src/content_table_source.rs'
            original = path.read_text()
            # Named mutation: content-ni-initial changes the packed parser key.
            # The same mutation is witnessed by oxpinyin-capi's
            # tests/it/keys.rs full_pinyin_one_key_surface.
            start = original.index('pinyin: "ni",')
            altered = original[:start] + original[start:].replace('initial: 11,', 'initial: 1,', 1)
            self.assertNotEqual(altered, original)
            path.write_text(altered, newline='\n')
            before = path.stat().st_mtime_ns
            with contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertEqual(rust_tables.regenerate(check=True, root=root), 1)
            key = path.relative_to(root).as_posix()  # the product keys its files with POSIX separators
            # Only the mutated file differs: no newline difference hides or fakes the drift.
            self.assertEqual(errors.getvalue().splitlines()[1:], [key])
            self.assertEqual(path.read_text(), altered)
            self.assertEqual(path.stat().st_mtime_ns, before)
            changed = dict(self.outputs, **{key: altered})
            with self.assertRaisesRegex(ValueError, 'CONTENT_TABLE'):
                rust_tables.verify_acceptance(changed)

    def test_default_and_explicit_modes_do_not_write_without_write_flag(self):
        # A write mode copies every output (regenerate's shutil.copyfile), so any write moves an mtime.
        committed = [rust_tables.ROOT / name for name in self.outputs]
        for argv in ([], ['--check']):
            before = {path: path.stat().st_mtime_ns for path in committed}
            with self.subTest(argv=argv), \
                    mock.patch('subprocess.run', side_effect=AssertionError('external command')), \
                    mock.patch('socket.socket', side_effect=AssertionError('network')), \
                    contextlib.redirect_stdout(io.StringIO()) as out:
                self.assertEqual(rust_tables.main(argv), 0)
                self.assertIn(f'{len(self.outputs)} files match', out.getvalue())
                self.assertEqual({path: path.stat().st_mtime_ns for path in committed}, before)

    def test_write_repairs_drift_in_an_explicit_destination(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.assertEqual(rust_tables.regenerate(check=False, root=root), 0)
            path = root / 'crates/oxpinyin-core/src/content_table_source.rs'
            path.write_text('drift')
            self.assertEqual(rust_tables.regenerate(check=False, root=root), 0)
            self.assertEqual(path.read_text(), self.outputs[path.relative_to(root).as_posix()])

    def test_pin_refresh_requires_explicit_write(self):
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                rust_tables.main(['--from-pin', 'anything'])
        self.assertEqual(error.exception.code, 2)

    def test_changed_upstream_module_and_input_fixture_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            source = Path(temp, 'source')
            shutil.copytree(rust_tables.SOURCE, source)
            module = source / 'upstream/scripts2/fullpinyin.py'
            original = module.read_bytes()
            module.write_bytes(original + b'\n')
            with self.assertRaisesRegex(ValueError, 'upstream blob mismatch.*fullpinyin.py'):
                rust_tables.verify_sources(source)
            module.write_bytes(original)
            fixture = source / 'inputs/pinyins.txt'
            fixture.write_bytes(fixture.read_bytes() + b'a 1\n')
            with self.assertRaisesRegex(ValueError, 'input fixture mismatch'):
                rust_tables.verify_sources(source)


if __name__ == '__main__':
    unittest.main()
