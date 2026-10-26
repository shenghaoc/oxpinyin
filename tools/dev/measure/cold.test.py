#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for APFS and Linux cold-page preparation and the shared rejection limit."""
import ast
import contextlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from unittest import mock
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import darwin
from tools.dev.measure import cold, cold_linux


class ColdTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.file = self.root / 'subject.bin'
        self.file.write_bytes(b'x' * (3 * darwin.PAGE_SIZE))

    def prepare_with(self, residency, files=None):
        """Run prepare over FILES with a fake mincore sequence and return its record."""
        files = [self.file] if files is None else files
        with mock.patch.object(cold, '_touch'), \
                mock.patch.object(cold.darwin, 'map_file', return_value=0x1000) as mapping, \
                mock.patch.object(cold.darwin, 'unmap'), \
                mock.patch.object(cold.darwin, 'msync') as sync, \
                mock.patch.object(cold.darwin, 'file_residency', side_effect=residency) as resident, \
                mock.patch.object(cold.os, 'fsync') as flush:
            record = cold.prepare(files)
        return record, mapping, sync, resident, flush

    def test_collect_resolves_symlinks_and_deduplicates_inodes(self):
        nested = self.root / 'data'
        nested.mkdir()
        (nested / 'one.bin').write_bytes(b'one')
        (nested / 'two.bin').write_bytes(b'two')
        (nested / 'alias.bin').symlink_to(nested / 'one.bin')
        os.link(nested / 'two.bin', nested / 'hard.bin')
        files = cold.collect([self.root, nested / 'alias.bin'])
        self.assertEqual(len(files), 3)
        self.assertEqual({path.name for path in files}, {'hard.bin', 'one.bin', 'subject.bin'})
        with self.assertRaises(FileNotFoundError):
            cold.collect([self.root / 'missing'])
        empty = self.root / 'empty'
        empty.mkdir()
        with self.assertRaisesRegex(ValueError, 'at least one regular file'):
            cold.collect([empty])

    def test_prepare_maps_read_only_shared_and_invalidates_before_checking(self):
        record, mapping, sync, resident, flush = self.prepare_with(
            [dict(pages=3, resident=0)])
        self.assertEqual(mapping.call_args.args[2:], (darwin.PROT_READ, darwin.MAP_SHARED))
        sync.assert_called_once_with(0x1000, self.file.stat().st_size,
                                     darwin.MS_SYNC | darwin.MS_INVALIDATE)
        flush.assert_called_once()
        resident.assert_called_once()
        self.assertEqual(record['metric'], 'darwin_resident_pages')
        self.assertEqual((record['value'], record['resident'], record['pages']), (0, 0, 3))
        self.assertEqual((record['attempts'], record['verdict']), (1, 'QUALIFIED'))
        self.assertEqual(record['files'][0]['path'], str(self.file))

    def test_prepare_retries_once_then_rejects_a_resident_set(self):
        with self.assertRaisesRegex(cold.Rejected, '2 of 3 darwin_resident_pages'):
            self.prepare_with([dict(pages=3, resident=2), dict(pages=3, resident=2)])
        record, mapping, sync, resident, _ = self.prepare_with(
            [dict(pages=3, resident=1), dict(pages=3, resident=0)])
        self.assertEqual((record['attempts'], record['verdict']), (2, 'QUALIFIED'))
        self.assertEqual([check['resident'] for check in record['checks']], [1, 0])
        self.assertEqual(sync.call_count, 2)
        self.assertEqual(mapping.call_count, 2)
        self.assertEqual(resident.call_count, 2)

    def test_prepare_prepares_the_whole_set_before_checking_it(self):
        other = self.root / 'other.bin'
        other.write_bytes(b'y')
        events = []

        def invalidate(fd):
            events.append(('prepare', os.fstat(fd).st_size))

        def check(handle, path):
            events.append(('check', path.name))
            return dict(path=str(path), pages=1, resident=0)

        with mock.patch.object(cold, '_invalidate', side_effect=invalidate), \
                mock.patch.object(cold, '_check', side_effect=check), \
                mock.patch.object(cold.os, 'fsync'):
            record = cold.prepare([self.file, other])
        self.assertEqual(record['verdict'], 'QUALIFIED')
        self.assertEqual(events, [('prepare', self.file.stat().st_size),
                                  ('prepare', 1), ('check', 'subject.bin'), ('check', 'other.bin')])

    def test_empty_file_has_zero_pages_and_no_mapping(self):
        empty = self.root / 'empty.bin'
        empty.write_bytes(b'')
        with mock.patch.object(cold.darwin, 'map_file') as mapping:
            record = cold.prepare([empty])
        self.assertEqual((record['pages'], record['resident'], record['attempts']), (0, 0, 1))
        mapping.assert_not_called()

    def test_rejected_preparation_carries_the_rejection_record(self):
        with self.assertRaises(cold.Rejected) as raised:
            self.prepare_with([dict(pages=3, resident=2), dict(pages=3, resident=1)])
        record = raised.exception.record
        self.assertEqual((record['metric'], record['verdict'], record['attempts']),
                         ('darwin_resident_pages', 'REJECTED', cold.ATTEMPTS))
        self.assertEqual((record['value'], record['resident'], record['pages']), (1, 1, 3))
        self.assertEqual([check['resident'] for check in record['checks']], [2, 1])
        self.assertEqual([row['path'] for row in record['files']], [str(self.file)])
        self.assertIn('1 of 3 darwin_resident_pages still resident after 2 preparations', str(raised.exception))
        self.assertIsNone(cold.Rejected('no record').record)

    def test_rejection_limit_is_the_maintainer_constant_never_computed(self):
        # Maintainer ruling 2026-10-25 UTC: 2 rejected samples per arm and metric, fixed before any sample.
        self.assertIs(type(cold.REJECTION_LIMIT), int)
        self.assertEqual(cold.REJECTION_LIMIT, 2)
        self.assertFalse(hasattr(cold, 'refusal'))
        # Structurally: one module-level binding, a literal, never reassigned from a function.
        tree = ast.parse(Path(cold.__file__).read_text())
        bindings = [node for node in ast.walk(tree) if isinstance(node, ast.Name)
                    and node.id == 'REJECTION_LIMIT' and not isinstance(node.ctx, ast.Load)]
        self.assertEqual(len(bindings), 1)
        assignment = next(node for node in tree.body if isinstance(node, ast.Assign)
                          and [t.id for t in node.targets if isinstance(t, ast.Name)] == ['REJECTION_LIMIT'])
        self.assertIsInstance(assignment.value, ast.Constant)
        self.assertEqual(assignment.value.value, 2)
        self.assertFalse([node for node in ast.walk(tree) if isinstance(node, (ast.Global, ast.Nonlocal))
                          and 'REJECTION_LIMIT' in node.names])

    def test_main_prints_the_darwin_metric_and_refuses_other_hosts(self):
        evidence = dict(metric=cold.METRIC, verdict='QUALIFIED', files=[])
        prepared = dict(metric=cold.METRIC, value=0, resident=0, pages=1,
                        attempts=1, verdict='QUALIFIED', checks=[], files=[])
        out = io.StringIO()
        with mock.patch.object(cold.sys, 'platform', 'darwin'), \
                mock.patch.object(cold, 'collect', return_value=[self.file]), \
                mock.patch.object(cold, 'demonstrate', return_value=evidence), \
                mock.patch.object(cold, 'prepare', return_value=prepared), \
                contextlib.redirect_stdout(out):
            self.assertEqual(cold.main([str(self.file)]), 0)
        self.assertEqual(json.loads(out.getvalue())['preparation']['metric'], cold.METRIC)
        with mock.patch.object(cold.sys, 'platform', 'linux'), \
                contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(cold.main([str(self.file)]), 1)
        self.assertIn('native macOS only', error.getvalue())

    def test_main_rejects_a_failed_control(self):
        with mock.patch.object(cold.sys, 'platform', 'darwin'), \
                mock.patch.object(cold, 'collect', return_value=[self.file]), \
                mock.patch.object(cold, 'demonstrate',
                                  return_value=dict(metric=cold.METRIC, verdict='REJECTED', files=[])), \
                mock.patch.object(cold, 'prepare') as prepare, \
                contextlib.redirect_stderr(io.StringIO()) as error:
            self.assertEqual(cold.main([str(self.file)]), 1)
        prepare.assert_not_called()
        self.assertIn('qualification control failed', error.getvalue())

    @unittest.skipUnless(sys.platform == 'darwin', 'Darwin mincore and APFS')
    def test_qualification_control_and_preparation_on_apfs(self):
        # The probe's own contract: before == pages, MS_SYNC == pages, and
        # MS_SYNC|MS_INVALIDATE == 0. Repeat on a private APFS file.
        path = self.root / 'control.bin'
        path.write_bytes(b'c' * (2 * 1024 * 1024))
        control = cold.demonstrate([path])
        self.assertEqual(control['verdict'], 'QUALIFIED')
        row = control['files'][0]
        self.assertEqual(row['before'], row['pages'])
        self.assertEqual(row['msync_sync'], row['pages'])
        self.assertEqual(row['after'], 0)
        self.assertGreater(row['pages'], 0)
        record = cold.prepare([path])
        self.assertEqual((record['metric'], record['value']), ('darwin_resident_pages', 0))
        self.assertEqual(record['verdict'], 'QUALIFIED')
        self.assertIn(record['attempts'], (1, 2))


class LinuxColdTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.facade = self.root / 'libpinyin.so.15.0.0'
        self.facade.write_bytes(b'f' * 8192)
        self.data = self.root / 'data'
        self.data.mkdir()
        (self.data / 'table.conf').write_bytes(b'conf')
        self.user = self.root / 'sample/user'
        self.user.mkdir(parents=True)
        (self.user / 'user.conf').write_bytes(b'user')
        (self.user / 'nested').mkdir()
        (self.user / 'nested/bigram.db').write_bytes(b'db' * 4096)
        self.files = cold.sample_files(self.facade, self.data, self.user)

    def prepare_with(self, residency, filesystem='ext4'):
        """Run cold_linux.prepare with fake fsync, eviction and mincore; return the record and event log."""
        events, answers = [], iter(residency)

        inodes = {(info.st_dev, info.st_ino): path.name
                  for path in self.files for info in [path.stat()]}

        def name(fd):
            """The basename of the file an open descriptor names, found by inode."""
            info = os.fstat(fd)
            return inodes[info.st_dev, info.st_ino]

        def resident(fd):
            events.append(('check', name(fd)))
            return dict(pages=2, resident=next(answers))

        with mock.patch.object(cold_linux.native, 'filesystem', return_value=(filesystem, '0:1')), \
                mock.patch.object(cold_linux.os, 'fsync', side_effect=lambda fd: events.append(('fsync', name(fd)))), \
                mock.patch.object(cold_linux, 'evict', side_effect=lambda fd: events.append(('evict', name(fd)))), \
                mock.patch.object(cold_linux.mman, 'residency', side_effect=resident):
            return cold_linux.prepare(self.files, self.user), events

    def test_user_copy_is_flushed_before_any_eviction_and_only_the_copy(self):
        record, events = self.prepare_with([0] * 4)
        flushed = [file for kind, file in events if kind == 'fsync']
        self.assertEqual(sorted(flushed), ['bigram.db', 'user.conf'])
        first_evict = next(i for i, (kind, _) in enumerate(events) if kind == 'evict')
        self.assertTrue(all(kind == 'fsync' for kind, _ in events[:first_evict]))
        self.assertEqual(len(events[:first_evict]), 2)
        # Every file is evicted before any is checked, so a refault cannot hide.
        self.assertEqual([kind for kind, _ in events[first_evict:]], ['evict'] * 4 + ['check'] * 4)
        self.assertEqual(record['metric'], 'linux_resident_pages')
        self.assertEqual((record['value'], record['resident'], record['pages']), (0, 0, 8))
        self.assertEqual((record['attempts'], record['verdict']), (1, 'QUALIFIED'))
        self.assertEqual(sorted(record), ['attempts', 'checks', 'files', 'metric', 'pages',
                                          'resident', 'value', 'verdict'])

    def test_a_resident_page_gets_one_more_eviction_and_check(self):
        record, events = self.prepare_with([0, 1, 0, 0, 0, 0, 0, 0])
        self.assertEqual((record['attempts'], record['verdict'], record['value']), (2, 'QUALIFIED', 0))
        self.assertEqual([check['resident'] for check in record['checks']], [1, 0])
        self.assertEqual([kind for kind, _ in events].count('evict'), 8)
        self.assertEqual([kind for kind, _ in events].count('fsync'), 2)

    def test_a_set_still_resident_after_the_second_eviction_is_rejected(self):
        with self.assertRaises(cold.Rejected) as raised:
            self.prepare_with([0, 1, 0, 0, 0, 0, 2, 0])
        self.assertIs(cold_linux.Rejected, cold.Rejected)
        record = raised.exception.record
        self.assertEqual((record['metric'], record['verdict'], record['attempts']),
                         ('linux_resident_pages', 'REJECTED', cold.ATTEMPTS))
        self.assertEqual((record['value'], record['resident'], record['pages']), (2, 2, 8))
        self.assertEqual([check['resident'] for check in record['checks']], [1, 2])
        self.assertIn('2 of 8 linux_resident_pages still resident after 2 evictions', str(raised.exception))
        self.assertNotIn('darwin', json.dumps(record))

    def test_a_file_off_ext4_is_refused_before_any_eviction(self):
        with self.assertRaisesRegex(ValueError, r'^cold cache requires ext4: .* is on tmpfs$'):
            self.prepare_with([], filesystem='tmpfs')

    def test_the_shared_limit_is_imported_not_redefined(self):
        tree = ast.parse(Path(cold_linux.__file__).read_text())
        self.assertFalse([node for node in ast.walk(tree) if isinstance(node, ast.Name)
                          and node.id == 'REJECTION_LIMIT'])
        self.assertFalse(hasattr(cold_linux, 'REJECTION_LIMIT'))
        self.assertNotEqual(cold_linux.METRIC, cold.METRIC)


if __name__ == '__main__':
    unittest.main()
