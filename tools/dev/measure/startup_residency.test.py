#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Native Linux checkpoint capture and shared post-round residency reporting."""
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import mman, test_support
from tools.dev.measure import cold, cold_linux, pages, startup, worker


class LinuxResidencyTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.library = self.root / 'libpinyin.so'
        self.library.write_bytes((ROOT / 'tools/dev/measure/fixtures/pages-elf-x86_64.so').read_bytes())
        self.data, self.profile, self.work = (self.root / name for name in ('data', 'user', 'work'))
        for path in (self.data, self.profile, self.work):
            path.mkdir()
        (self.data / 'system.bin').write_bytes(b'd' * (mman.PAGE_SIZE + 3))
        (self.profile / 'user.bin').write_bytes(b'u')
        (self.root / 'profile.manifest').write_text(worker.startup_profile.manifest(self.profile))
        self.point = None
        self.reads = []
        self.prepared = dict(metric=cold_linux.METRIC, value=0, verdict='QUALIFIED')
        self.layout = self.enterContext(mock.patch.object(pages, 'object_layout', wraps=pages.object_layout))
        self.enterContext(mock.patch.object(worker, 'sys', SimpleNamespace(platform='linux')))
        self.enterContext(mock.patch.object(worker, 'check_identity'))
        self.enterContext(mock.patch.object(worker, 'exports', return_value=[]))
        self.prepare = self.enterContext(mock.patch.object(cold_linux, 'prepare', return_value=self.prepared))
        self.execute = self.enterContext(mock.patch.object(worker, 'execute', side_effect=self.run_checkpoints))
        self.reader = self.enterContext(mock.patch.object(mman, 'vector', side_effect=self.vector))
        self.campaign = worker.Worker(Path('unused-worker'), {}, self.data, self.work, {}, (), None, 'now')

    def vector(self, fd):
        """Fake Linux mincore: retain the descriptor identity and scripted checkpoint pages."""
        info = os.fstat(fd)
        self.reads.append((self.point, info.st_dev, info.st_ino))
        marks = bytearray(-(-info.st_size // mman.PAGE_SIZE))
        if self.point != 'start':
            marks[0] = 1
        if self.point in ('init', 'config', 'alloc', 'first_candidate', 'end') and len(marks) > 1:
            marks[1] = 1
        return bytes(marks)

    def run_checkpoints(self, command, **options):
        """Drive the real capture callback at every worker pause, without starting a child."""
        test_support.assert_not_called(self, self.layout)
        if options['memory'] and 'residency' in options:
            vectors = {}
            for self.point in worker.POINTS:
                vectors[self.point] = options['residency']()
            result = dict(residency=vectors)
        else:
            self.assertNotIn('residency', options)
            result = {}
        test_support.assert_not_called(self, self.layout)
        return dict(result, status=0, stdout=b'{"startup":"units","stopped_at":"end"}\n',
                    stderr=b'', process_wall=1, checkpoints={})

    def sample(self, **options):
        row = self.campaign.run(self.library, 'pinyin', self.profile, **options)
        return dict(row, round=7)

    def report(self, sample):
        output = io.StringIO()
        with mock.patch.object(startup, 'emit', side_effect=lambda kind, **values:
                               output.write(json.dumps(dict(startup=kind, **values)) + '\n')):
            startup.report_page_residency('pinyin', 'empty', 'pr', [sample], metric=cold_linux.METRIC)
        records = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertNotIn(cold.METRIC, output.getvalue())
        return records

    def test_linux_cold_memory_captures_each_file_and_reports_elf_sections_symbols_and_offsets(self):
        sample = self.sample(cold=True, memory=True)
        self.assertEqual(tuple(sample['residency']), worker.POINTS)
        self.assertEqual(self.reader.call_count, 3 * len(worker.POINTS))
        for point, readings in sample['residency'].items():
            self.assertEqual(set(readings), {'object', 'data/system.bin', 'user/user.bin'})
            expected = {(point, *reading['identity']) for reading in readings.values()}
            self.assertEqual({read for read in self.reads if read[0] == point}, expected)
        test_support.assert_not_called(self, self.layout)
        records = self.report(sample)
        self.assertEqual([r['checkpoints'] for r in records], [list(pair) for pair in pages.checkpoint_pairs(worker.POINTS)])
        self.assertTrue(all(r['round'] == 7 and r['page_size'] == mman.PAGE_SIZE
                            and r['metric'] == cold_linux.METRIC and r['limits'] == list(pages.LIMITS)
                            for r in records))
        entry = records[0]['files']['object']
        self.assertEqual(entry[cold_linux.METRIC], 1)
        self.assertNotIn('layout_error', entry)
        self.assertIn('.text', entry['rows'][0]['elf_sections'])
        self.assertIn('.rodata', entry['rows'][0]['elf_sections'])
        self.assertIn('pages_first', entry['rows'][0]['elf_symbols'])
        self.assertIn('pages_table', entry['rows'][0]['elf_symbols'])
        self.assertEqual(records[1]['files']['data/system.bin']['rows'], [dict(
            file='data/system.bin', offset_range=[mman.PAGE_SIZE, mman.PAGE_SIZE + 3])])
        self.assertEqual(records[0]['files']['user/user.bin']['rows'][0]['offset_range'], [0, 1])
        self.assertTrue(all(not r['files'] for r in records[2:]))
        self.assertEqual(self.layout.call_count, 1)

    def test_linux_replaced_object_falls_back_with_a_reason(self):
        sample = self.sample(cold=True, memory=True)
        replacement = self.library.with_name('replacement.so')
        replacement.write_bytes(self.library.read_bytes())
        replacement.replace(self.library)
        entry = self.report(sample)[0]['files']['object']
        self.assertIn('object replaced or modified before parsing', entry['layout_error'])
        self.assertEqual(entry[cold_linux.METRIC], 1)
        self.assertEqual(entry['rows'], [dict(file='object',
                                            offset_range=[0, min(mman.PAGE_SIZE, self.library.stat().st_size)])])
        test_support.assert_not_called(self, self.layout)

    def test_linux_timing_and_warm_memory_do_not_capture_vectors(self):
        for options in (dict(cold=True), dict(memory=True), dict()):
            with self.subTest(options=options):
                self.assertNotIn('residency', self.sample(**options))
        test_support.assert_not_called(self, self.reader)
        test_support.assert_not_called(self, self.layout)

    def test_linux_rejected_preparation_does_not_capture_or_parse(self):
        self.prepare.side_effect = cold.Rejected('synthetic rejection', dict(verdict='REJECTED'))
        with self.assertRaises(cold.Rejected):
            self.sample(cold=True, memory=True)
        test_support.assert_not_called(self, self.execute)
        test_support.assert_not_called(self, self.reader)
        test_support.assert_not_called(self, self.layout)
        self.assertEqual(list(self.work.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
