"""ELF observations that tools/dev/runner_recipes.py reads for `just cpp-smoke`; no Linux host needed."""
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev.runner_recipes import pinyin_needed, pinyin_resolved


class ElfObservationTests(unittest.TestCase):
    def test_elf_observations_are_parsed_without_a_linux_host(self):
        self.assertEqual(pinyin_needed(' (NEEDED) Shared library: [libpinyin.so.15]\n Shared library: [libc.so.6]'), 'libpinyin.so.15')
        self.assertEqual(pinyin_resolved('libpinyin.so.15 => /build/lib/libpinyin.so.15 (0x1)'), '/build/lib/libpinyin.so.15')
        self.assertEqual(pinyin_needed('Shared library: [liboxpinyin_capi.so]'), 'liboxpinyin_capi.so')


if __name__ == '__main__':
    unittest.main()
