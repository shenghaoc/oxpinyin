#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The w3 fixture recipe: the subset, the build table.conf, the comparison and the commands."""
import contextlib
import hashlib
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import model, w3_fixtures

GB_CHAR = ("a\t吖\t16777217\t104\n"
           "ya\t吖\t16777217\t793\n"
           "b\t吧\t16777218\t5\n"
           "ni\t你\t16777219\t7\n"
           "ni'hao\t你好\t16777220\t9\n"
           "\n"
           "# a comment\n"
           "hao\t好\t16777221\t-3\n"
           "hao\t好\t16777221\t11\n")
ART = ("er'huang\t二簧\t67108865\t100\n"
       "er'hu\t二胡\t67108866\t50\n")
INTERPOLATION = ("\\data model interpolation\n"
                 "\\1-gram\n"
                 "\\item 16777217 吖 count 9\n"
                 "\\item 16777218 吧 count 4\n"
                 "\\item 1 <start> count 99\n"
                 "\\2-gram\n"
                 "\\item 16777217 吖 16777218 吧 count 5\n"
                 "\\item 16777217 吖 16777219 你 count 3\n"
                 "\\item 16777218 吧 16777217 吖 count 7\n"
                 "\\item 1 <start> 16777219 你 count 2\n"
                 "\\end\n")


def write_model(directory):
    """A model20-shaped directory: the four system tables, art, punct and the interpolation."""
    directory.mkdir(parents=True, exist_ok=True)
    for name in model.EXPECTED_MODEL_FILES:
        (directory / name).write_text('')
    (directory / 'gb_char.table').write_text(GB_CHAR, encoding='utf-8')
    (directory / 'art.table').write_text(ART, encoding='utf-8')
    (directory / 'interpolation2.text').write_text(INTERPOLATION, encoding='utf-8')
    (directory / 'punct.table').write_text('16777217 吖 ， 10\n', encoding='utf-8')
    (directory.parent / 'verified').write_text(f'sha256={model.MODEL20_SHA256}\n')
    return directory


class SubsetTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='oxpinyin-w3-test-')
        self.addCleanup(scratch.cleanup)
        self.work = Path(scratch.name)
        self.model = write_model(self.work / 'cache' / 'extracted')
        self.out = self.work / 'subset'
        self.kept, self.kept_art = w3_fixtures.subset(self.model, self.out)

    def test_a_kept_token_keeps_every_row_and_an_unlisted_spelling_drops_its_phrase(self):
        # "a" is a mini key, so 吖 survives with both its readings; 吧 is
        # reachable through "b" alone and goes; the negative-count row of
        # 好 is skipped as the pin skips it, the comment and blank line too.
        self.assertEqual(self.kept, {16777217, 16777219, 16777220, 16777221})
        self.assertEqual((self.out / 'gb_char.table').read_text(encoding='utf-8'),
                         "a\t吖\t16777217\t104\nya\t吖\t16777217\t793\nni\t你\t16777219\t7\n"
                         "ni'hao\t你好\t16777220\t9\nhao\t好\t16777221\t11\n")
        for name in ('gbk_char', 'opengram', 'merged'):
            self.assertEqual((self.out / f'{name}.table').read_text(), '')

    def test_art_keeps_the_art_keys_only(self):
        self.assertEqual(self.kept_art, {67108865})
        self.assertEqual((self.out / 'art.table').read_text(encoding='utf-8'), "er'huang\t二簧\t67108865\t100\n")

    def test_the_interpolation_keeps_kept_unigrams_and_two_sided_bigrams(self):
        # 吧's unigram and every row naming 吧 or <start> go; the headers,
        # the section lines and \end pass through in place.
        self.assertEqual((self.out / 'interpolation2.text').read_text(encoding='utf-8'),
                         "\\data model interpolation\n\\1-gram\n\\item 16777217 吖 count 9\n"
                         "\\2-gram\n\\item 16777217 吖 16777219 你 count 3\n\\end\n")

    def test_the_punct_table_is_copied_whole(self):
        self.assertEqual((self.out / 'punct.table').read_bytes(), (self.model / 'punct.table').read_bytes())

    def test_more_than_four_fields_is_refused_with_the_line(self):
        (self.model / 'gb_char.table').write_text('a\t吖\t16777217\t104\textra\n')
        with self.assertRaises(w3_fixtures.Refused) as caught:
            w3_fixtures.subset(self.model, self.out)
        self.assertIn('gb_char.table:1: more than four fields', str(caught.exception))

    def test_an_item_line_outside_its_section_shape_is_refused(self):
        (self.model / 'interpolation2.text').write_text('\\data model interpolation\n\\1-gram\n\\item 1 2 3 4 5 6\n')
        with self.assertRaises(w3_fixtures.Refused):
            w3_fixtures.subset(self.model, self.out)


class TableConfTests(unittest.TestCase):
    def test_the_committed_rendering_is_the_pins_with_the_backend_filled_in(self):
        for name, token in (('db', 'BerkeleyDB'), ('kct', 'KyotoCabinet'), ('tkt', 'Tkrzw')):
            with self.subTest(name=name):
                text = w3_fixtures.table_conf(token)
                self.assertIn(f'database format:{token}\n', text)
                self.assertEqual(text, (ROOT / 'fixtures/w3' / name / 'table.conf').read_text())
                self.assertEqual(sum(line.startswith('addon ') for line in text.splitlines()), 12)

    def test_the_build_rendering_lists_art_alone(self):
        text = w3_fixtures.table_conf('Tkrzw', build=True)
        addons = [line for line in text.splitlines() if line.startswith('addon ')]
        self.assertEqual(addons, ['addon 4 art.table art.bin NULL DICTIONARY'])
        self.assertIn('default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n', text)


class CompareTests(unittest.TestCase):
    """The comparison over a fake record reader that prints the file's own lines."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='oxpinyin-w3-compare-')
        self.addCleanup(scratch.cleanup)
        self.work = Path(scratch.name)
        self.reader = self.work / 'raw_records'
        self.reader.write_text('#!/bin/sh\ncat "$2"\n')
        self.reader.chmod(0o755)
        self.produced, self.committed = self.work / 'produced', self.work / 'committed'
        for directory in (self.produced, self.committed):
            directory.mkdir()
            (directory / 'gb_char.bin').write_bytes(b'chunk')
            (directory / 'table.conf').write_text('conf')
            (directory / 'pinyin_index.bin').write_text('00 aa\n01 bb\n')
            (directory / 'bigram.db').write_text('02 cc\n')
        (self.committed / 'k_mixture_model.db').write_bytes(b'pinned')

    def statuses(self):
        return {name: (status, detail) for name, status, detail in
                w3_fixtures.compare(self.reader, self.produced, self.committed)}

    def test_identical_trees_report_bytes_and_records(self):
        got = self.statuses()
        self.assertEqual(got['gb_char.bin'], ('identical', '5 bytes'))
        self.assertEqual(got['pinyin_index.bin'], ('identical', '2 records'))
        self.assertEqual(got['bigram.db'], ('identical', '1 records'))
        self.assertEqual(got['k_mixture_model.db'][0], 'pinned')

    def test_a_changed_record_names_its_row_and_key(self):
        (self.produced / 'pinyin_index.bin').write_text('00 aa\n01 bc\n')
        status, detail = self.statuses()['pinyin_index.bin']
        self.assertEqual(status, 'differ')
        self.assertIn('row 1 differs (key 01 vs 01)', detail)

    def test_a_missing_or_extra_file_differs(self):
        (self.produced / 'extra.bin').write_bytes(b'')
        (self.produced / 'gb_char.bin').unlink()
        got = self.statuses()
        self.assertEqual(got['extra.bin'], ('differ', 'produced, but not in the committed tree'))
        self.assertEqual(got['gb_char.bin'], ('differ', 'committed, but the recipe does not produce it'))

    def test_a_changed_chunk_reports_both_sizes(self):
        (self.produced / 'gb_char.bin').write_bytes(b'chunk2')
        self.assertEqual(self.statuses()['gb_char.bin'], ('differ', '6 produced vs 5 committed bytes'))

    def test_the_report_counts_and_flags_a_difference(self):
        (self.produced / 'bigram.db').write_text('02 cd\n')
        settings = mock.Mock(extension='tkt', cell='tkrzw')
        out = io.StringIO()
        differs = w3_fixtures.report(settings, w3_fixtures.compare(self.reader, self.produced, self.committed), out)
        self.assertTrue(differs)
        self.assertIn('tkt/bigram.db: differ', out.getvalue())
        self.assertIn('tkrzw (tkt): 3 identical, 1 differ, 1 pinned\n', out.getvalue())


class ChecksumTests(unittest.TestCase):
    def test_every_file_is_listed_in_byte_order_with_the_readme_prefix(self):
        with tempfile.TemporaryDirectory() as scratch:
            fixtures = Path(scratch)
            (fixtures / 'tkt').mkdir()
            (fixtures / 'tkt' / 'b.bin').write_bytes(b'b')
            (fixtures / 'a.bin').write_bytes(b'a')
            (fixtures / 'fixtures.sha256').write_text('stale')
            w3_fixtures.write_checksums(fixtures)
            lines = (fixtures / 'fixtures.sha256').read_text().splitlines()
        self.assertEqual(lines, [f'{hashlib.sha256(b"a").hexdigest()}  fixtures/w3/a.bin',
                                 f'{hashlib.sha256(b"b").hexdigest()}  fixtures/w3/tkt/b.bin'])

    def test_the_committed_checksums_match_the_tree(self):
        fixtures = ROOT / 'fixtures/w3'
        with tempfile.TemporaryDirectory() as scratch:
            copy = Path(scratch) / 'w3'
            import shutil
            shutil.copytree(fixtures, copy)
            w3_fixtures.write_checksums(copy)
            self.assertEqual((copy / 'fixtures.sha256').read_text(), (fixtures / 'fixtures.sha256').read_text())


class ModelDirTests(unittest.TestCase):
    def test_an_incomplete_or_unverified_directory_is_refused(self):
        with tempfile.TemporaryDirectory() as scratch:
            extracted = Path(scratch) / 'cache' / 'extracted'
            with self.assertRaises(w3_fixtures.Refused) as caught:
                w3_fixtures.model_dir(str(extracted), {})
            self.assertEqual(caught.exception.status, 2)
            self.assertIn('not a complete model20 extraction', str(caught.exception))
            write_model(extracted)
            self.assertEqual(w3_fixtures.model_dir(str(extracted), {}), extracted.resolve())
            (extracted.parent / 'verified').write_text('sha256=0000\n')
            with self.assertRaises(w3_fixtures.Refused) as caught:
                w3_fixtures.model_dir(None, {'PINYIN_MODEL_DIR': str(extracted)})
            self.assertIn('does not record the pinned archive digest', str(caught.exception))


class CommandTests(unittest.TestCase):
    def main(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = w3_fixtures.main(list(argv), out, err)
        return status, out.getvalue(), err.getvalue()

    def test_usage_errors_and_help(self):
        self.assertEqual(self.main()[0], 2)
        self.assertEqual(self.main('--bogus')[0], 2)
        status, out, _ = self.main('check', '--help')
        self.assertEqual(status, 0)
        self.assertIn('--model-dir', out)

    def test_an_unknown_cell_is_refused_before_any_build(self):
        with tempfile.TemporaryDirectory() as scratch:
            extracted = write_model(Path(scratch) / 'cache' / 'extracted')
            status, _, err = self.main('check', '--model-dir', str(extracted), '--cells', 'tkrzw,lmdb')
        self.assertEqual(status, 2)
        self.assertIn("unknown cell 'lmdb'", err)

    def test_content_hash_only_lists_the_committed_content_tables(self):
        status, out, _ = self.main('content', '--hash-only')
        self.assertEqual(status, 0)
        self.assertEqual(len(out.splitlines()), 12)
        self.assertTrue(out.startswith(hashlib.sha256((ROOT / 'fixtures/w3/art.bin').read_bytes()).hexdigest()))

    def test_content_refuses_a_missing_data_directory(self):
        status, _, err = self.main('content', '--data-dir', '/absent/data')
        self.assertEqual(status, 1)
        self.assertIn('data directory not found', err)

    def test_content_truncates_the_same_bytes_the_committed_tables_hold(self):
        # The committed art.bin is its own 151-record source truncated to 100 records
        # or fewer; re-truncating it must leave it as it is.
        committed = (ROOT / 'fixtures/w3/art.bin').read_bytes()
        truncated, keep, original = w3_fixtures.truncate_content(committed, 100)
        self.assertEqual(keep, original)
        self.assertEqual(truncated, committed)


if __name__ == '__main__':
    unittest.main()
