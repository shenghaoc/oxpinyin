#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Tests for the GLib/libc call-site generator."""
import csv
import importlib.util
import sys
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

SPEC = importlib.util.spec_from_file_location(
    "glib_call_sites", Path(__file__).with_name("glib_call_sites.py"))
driver = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(driver)

FIXTURE = Path(__file__).with_name("fixtures") / "small"
COMMITTED = ROOT / "docs/findings/glib-libc-call-sites.tsv"
EXPECTED = (
    "family\tlibrary\tfacade\tbackend\tpin_site\tpin_call\tchild\treach\tcall\n"
    "fixture\tglib\tboth\tall\tsrc/pinyin.cpp:2\tg_utf8_get_char\t#900\texported\t"
    '"g_utf8_get_char(""\\\\"")"\n'
    "fixture\tglib\tboth\tall\tsrc/pinyin.cpp:4\tg_strdup\t#900\tdead\t"
    '"g_strdup(""dead"")"\n'
    "fixture\tlibc\tboth\tall\tsrc/pinyin.cpp:6\tfscanf\t#900\tmanual\t"
    '"fscanf(input, ""%d"", &value)"\n'
    "fixture\tglib\tboth\tall\tsrc/storage/tag_utility.cpp:2\tg_free\t#900\ttool-only\t"
    "g_free(pointer)\n"
)


class CallSiteTests(unittest.TestCase):
    def test_the_fixture_is_reproduced_byte_for_byte(self):
        rows = driver.build_rows(
            FIXTURE, driver.read_classification(FIXTURE / "classification.tsv"))
        self.assertEqual(driver.render(rows), EXPECTED)

    def test_the_committed_tsv_parses_with_one_column_count(self):
        with COMMITTED.open(newline="") as stream:
            rows = list(csv.reader(stream, delimiter="\t", strict=True))
        self.assertEqual(rows[0], list(driver.HEADER))
        self.assertGreater(len(rows), 1)
        self.assertEqual({len(row) for row in rows}, {len(driver.HEADER)})
        self.assertNotIn("\r", COMMITTED.read_text())

    def test_the_pin_check_rejects_another_commit(self):
        with mock.patch("subprocess.check_output", return_value=driver.PIN + "\n"):
            driver.check_pin(Path("."))
        with mock.patch("subprocess.check_output", return_value="0" * 40 + "\n"):
            with self.assertRaises(ValueError):
                driver.check_pin(Path("."))

    def test_a_missing_classification_is_an_error(self):
        with self.assertRaises(ValueError):
            driver.build_rows(FIXTURE, {})


if __name__ == "__main__":
    unittest.main()
