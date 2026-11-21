"""Test the Callgrind .text comparison. The real-tool checks use shared objects built on Linux."""
import contextlib
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import neutrality


class CompareTests(unittest.TestCase):
    def compare_bytes(self, first, second):
        sections = {'a.so': first, 'b.so': second}
        sources = []

        def objcopy(command, *, capture_output):
            tool, option, output_format, section, source, destination = command
            self.assertEqual((tool, option, output_format, section), ('objcopy', '-O', 'binary', '--only-section=.text'))
            sources.append(source)
            Path(destination).write_bytes(sections[source])
            return subprocess.CompletedProcess(command, 0)

        result = neutrality.compare('a.so', 'b.so', run=objcopy)
        self.assertEqual(sources, ['a.so', 'b.so'] if first else ['a.so'])
        return result

    def test_identical_bytes_pass(self):
        digest = hashlib.sha256(b'same').hexdigest()
        self.assertEqual(self.compare_bytes(b'same', b'same'),
                         (0, f'with debug info:    .text sha256 {digest} (4 bytes)\n'
                             f'without debug info: .text sha256 {digest} (4 bytes)\n'
                             'PASS: identical .text bytes\n'))

    def test_empty_text_names_the_library(self):
        for first, second, library in ((b'', b'code', 'a.so'), (b'code', b'', 'b.so'), (b'', b'', 'a.so')):
            with self.subTest(first=first, second=second):
                self.assertEqual(self.compare_bytes(first, second), (1, f'FAIL: {library} has an empty .text section\n'))

    def test_different_bytes_of_equal_length_fail(self):
        first, second = b'ab', b'ba'
        self.assertEqual(self.compare_bytes(first, second),
                         (1, f'with debug info:    .text sha256 {hashlib.sha256(first).hexdigest()} (2 bytes)\n'
                             f'without debug info: .text sha256 {hashlib.sha256(second).hexdigest()} (2 bytes)\n'
                             'FAIL: .text bytes differ — neutrality unproven. '
                             'Diff the objdump -d output of the two libraries by hand.\n'))

    def test_objcopy_failure_returns_its_shell_status(self):
        for code, expected in ((3, 3), (-9, 137)):
            with self.subTest(code=code):
                run = mock.Mock(return_value=subprocess.CompletedProcess([], code))
                self.assertEqual(neutrality.compare('a.so', 'b.so', run=run), (expected, ''))
                run.assert_called_once()


if sys.platform.startswith('linux'):
    class RealToolsTests(unittest.TestCase):
        """Shared objects compiled here, and the real tools: every outcome, on whatever architecture this is."""

        BASE = '''static int table[4] = {1, 2, 3, 4};
int __attribute__((noinline)) f(int x) { return x * 3 + 1; }
int __attribute__((noinline)) g(int y) { return f(y) + 7; }
int _RNvCsabc123_5crate3foo(int z) { return z ^ 0x55; }
int %(name)s(int q) { return q + 40; }
int get(int i) { return table[i & 3]; }
'''

        @classmethod
        def setUpClass(cls):
            cls.directory = tempfile.TemporaryDirectory()
            cls.addClassCleanup(cls.directory.cleanup)
            work = Path(cls.directory.name)
            base = cls.BASE % {'name': 'h_same'}
            sources = {'base': base, 'base_g': base,
                       'different': base.replace('x * 3 + 1', '(x >> 2) ^ 1')}
            cls.objects = {}
            for name, source in sources.items():
                (work / f'{name}.c').write_text(source)
                flags = ['-g'] if name == 'base_g' else []
                subprocess.run(['gcc', '-O2', *flags, '-shared', '-fPIC', '-o', work / f'{name}.so', work / f'{name}.c'],
                               check=True, capture_output=True)
                cls.objects[name] = work / f'{name}.so'

        def compare(self, first, second):
            return neutrality.compare(self.objects[first], self.objects[second])

        def test_campaign_control_checks_dwarf_and_shipping_code_without_mutation(self):
            from tools.dev.measure.valgrind import callgrind_control
            with tempfile.TemporaryDirectory() as directory:
                stages = [Path(directory) / tag for tag in ('profile', 'shipping')]
                originals = []
                for stage, name in zip(stages, ('base_g', 'base')):
                    stage.mkdir()
                    library = stage / 'libpinyin.so.15.0.0'
                    shutil.copy(self.objects[name], library)
                    originals.append(library.read_bytes())
                    (stage / 'libpinyin.so').symlink_to(library.name)
                with io.TextIOWrapper(io.BytesIO(), encoding='utf-8') as stream, contextlib.redirect_stdout(stream):
                    status = callgrind_control(list(map(str, stages)), os.environ)
                    report = stream.buffer.getvalue().decode()
                self.assertEqual(status, 0, report)
                self.assertIn('PASS: identical .text bytes', report.splitlines())
                self.assertRegex(report, r'profiled DWARF compilation units: [1-9][0-9]*')
                for stage, original in zip(stages, originals):
                    self.assertEqual((stage / 'libpinyin.so').read_bytes(), original)
                    self.assertTrue((stage / 'libpinyin.so').is_symlink())

        def test_different_code_fails(self):
            status, report = self.compare('base', 'different')
            self.assertEqual(status, 1, report)
            self.assertIn('neutrality unproven', report)
            self.assertIn('objdump -d', report)


if __name__ == '__main__':
    unittest.main()
