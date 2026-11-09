#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The w3 subset recipe: the subset, the table.conf renderings and the model directory."""
from pathlib import Path
import sys
import tempfile
import unittest

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


if __name__ == '__main__':
    unittest.main()
