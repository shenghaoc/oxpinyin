#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for per-page residency attribution to sections, symbols and offsets.

The object fixtures in tools/dev/measure/fixtures/ were built once from
pages.c by build-pages.py in that directory, on macOS 27.0.1 (Apple M5):
the dylibs with Apple clang 21.0.0 (clang-2100.3.34.2), ld-27037.1 and the
macOS 27.0 SDK, the ELF object with clang and LLD 21.0.0 (swiftlang
903b9faa):

    uv run --no-project --managed-python python -X lazy_imports=normal \\
        tools/dev/measure/fixtures/build-pages.py --elf-cc /path/to/clang

FIXTURES records their sizes and SHA-256; a rebuild with the same toolchain
reproduced them. Expected offsets come from GNU readelf 2.47 (-SW, -lW, -sW)
for the ELF object and from otool -l and nm -m for the dylibs, not from
pages.py. Residency vectors come from a fake reader, never from mincore.
"""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import pages

FIXTURE_DIR = ROOT / 'tools/dev/measure/fixtures'
FIXTURES = {
    'pages-elf-x86_64.so': (3848, '156577c12805ecfa2b37d0a96012c1b38817fb9338a24c0f86d72371eb5dc172'),
    'pages-macho-arm64.dylib': (33560, '5b39b1f698e0a24f5ac425fe447f87cd79943ef08fe4c4bec1f0615de6b840b8'),
    'pages-macho-x86_64.dylib': (8568, 'dd71ef847991bce7daa6c10a8fccedcb7740e4220b546723340c9827fdf16775'),
}
ELF = FIXTURE_DIR / 'pages-elf-x86_64.so'
ARM64 = FIXTURE_DIR / 'pages-macho-arm64.dylib'
X86_64 = FIXTURE_DIR / 'pages-macho-x86_64.dylib'
ELF_FIELDS = {'file', 'page_offset', 'elf_sections', 'elf_symbols'}
MACHO_FIELDS = {'file', 'page_offset', 'macho_segments', 'macho_sections', 'macho_symbols'}


class FakeResidency:
    """Stand in for a mincore reader: return each file's vector at a checkpoint.

    The vectors may set flag bits above bit 0, as Darwin's mincore does.
    """
    def __init__(self, vectors):
        self.vectors = vectors
        self.reads = []

    def __call__(self, checkpoint, path):
        self.reads.append((checkpoint, path))
        return bytes(self.vectors[checkpoint][path])


def pages_at(path, offsets, page_size):
    """Map the given page offsets of an object fixture, keyed by offset."""
    rows = pages.object_rows(pages.object_layout(path), offsets, page_size, path.name)
    return {row['page_offset']: row for row in rows}


class PagesTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def variant(self, source, edits=(), size=None):
        """Write a new copy of SOURCE with byte EDITS and an optional truncation."""
        data = bytearray(source.read_bytes()[:size])
        for offset, value in edits:
            data[offset:offset + len(value)] = value
        path = self.root / f'variant-{len(list(self.root.iterdir()))}-{source.name}'
        path.write_bytes(data)
        return path

    def test_fixtures_are_the_recorded_bytes(self):
        for name, (size, digest) in FIXTURES.items():
            data = (FIXTURE_DIR / name).read_bytes()
            self.assertEqual((len(data), hashlib.sha256(data).hexdigest()), (size, digest), name)

    def test_newly_resident_reads_bit_zero_and_counts_growth(self):
        read = FakeResidency({'M0': {'f': [0x01, 0x00, 0x02, 0x81, 0x00]},
                              'I1': {'f': [0x01, 0x01, 0x03, 0x80, 0x7e, 0x01, 0x00]}})
        before, after = read('M0', 'f'), read('I1', 'f')
        # Page 1 becomes resident, page 2 gains bit 0 beside a flag, page 3
        # loses it, page 4 has flags only, and page 5 lies past the end of the
        # earlier, shorter vector.
        self.assertEqual(pages.newly_resident(before, after, 4096), [4096, 2 * 4096, 5 * 4096])
        self.assertEqual(pages.newly_resident(after, before, 4096), [3 * 4096])
        self.assertEqual(read.reads, [('M0', 'f'), ('I1', 'f')])

    def test_page_size_must_be_a_positive_power_of_two(self):
        for size in (0, -4096, 3000, 4096.0, True):
            with self.assertRaisesRegex(ValueError, 'power of two'):
                pages.newly_resident(b'', b'', size)
        with self.assertRaisesRegex(ValueError, 'power of two'):
            pages.data_rows([0], 48, 100, 'data')

    def test_elf_page_to_section_and_symbol(self):
        rows = pages_at(ELF, [0x0, 0x440, 0x6a0], 32)
        self.assertEqual(rows[0x440], dict(file=ELF.name, page_offset=0x440,
                                           elf_sections=['.rodata'], elf_symbols=['pages_table']))
        # The ELF header belongs to no section.
        self.assertEqual((rows[0x0]['elf_sections'], rows[0x0]['elf_symbols']), ([], []))
        # .relro_padding is NOBITS at 0x6a8 and holds no file bytes.
        self.assertEqual(rows[0x6a0]['elf_sections'], ['.got', '.data', '.got.plt'])
        self.assertEqual(rows[0x6a0]['elf_symbols'], ['pages_counter'])

    def test_verified_bytes_are_parsed_without_opening_the_diagnostic_path(self):
        missing = self.root / 'not-opened'
        for path in (ELF, ARM64, X86_64):
            with self.subTest(object=path.name):
                self.assertEqual(pages.object_layout(missing, data=path.read_bytes()), pages.object_layout(path))

    def test_elf_pages_spanning_sections_and_symbols(self):
        rows = pages_at(ELF, [0x520, 0x540, 0x5a0], 32)
        self.assertEqual(rows[0x520]['elf_sections'], ['.eh_frame', '.text'])
        self.assertEqual(rows[0x520]['elf_symbols'], ['pages_first'])
        self.assertEqual(rows[0x540]['elf_sections'], ['.text'])
        self.assertEqual(rows[0x540]['elf_symbols'], ['pages_first', 'pages_second', 'pages_third'])
        # pages_tls's st_value is 0 relative to PT_TLS (0x25b0), so it sits at
        # .tdata's 0x5b0; the zero-size _DYNAMIC at 0x5b8 is left out.
        self.assertEqual(rows[0x5a0]['elf_sections'], ['.plt', '.tdata', '.dynamic'])
        self.assertEqual(rows[0x5a0]['elf_symbols'], ['pages_tls'])

    def test_elf_host_page_lists_every_section_and_sized_symbol(self):
        row = pages_at(ELF, [0], 4096)[0]
        self.assertEqual(set(row), ELF_FIELDS)
        self.assertEqual(row['elf_sections'], [
            '.dynsym', '.gnu.hash', '.dynstr', '.rela.dyn', '.rela.plt', '.rodata', '.eh_frame_hdr',
            '.eh_frame', '.text', '.plt', '.tdata', '.dynamic', '.got', '.data', '.got.plt',
            '.comment', '.symtab', '.shstrtab', '.strtab'])
        self.assertEqual(row['elf_symbols'], ['pages_table', 'pages_first', 'pages_second', 'pages_third',
                                              'pages_tls', 'pages_counter'])

    def test_macho_arm64_pages_to_segments_sections_and_symbols(self):
        rows = pages_at(ARM64, [0x0, 0x4000, 0x8000], 16384)
        self.assertEqual(rows[0x0], dict(
            file=ARM64.name, page_offset=0x0, macho_segments=['__TEXT'],
            macho_sections=['__TEXT,__text', '__TEXT,__const', '__TEXT,__unwind_info'],
            macho_symbols=['_pages_first', '_pages_second', '_pages_third', '_pages_table']))
        self.assertEqual(rows[0x4000]['macho_segments'], ['__DATA'])
        self.assertEqual(rows[0x4000]['macho_sections'],
                         ['__DATA,__data', '__DATA,__thread_vars', '__DATA,__thread_data'])
        self.assertEqual(rows[0x4000]['macho_symbols'],
                         ['_pages_counter', '_pages_tls', '_pages_tls$tlv$init'])
        # __LINKEDIT has no sections, and the undefined __tlv_bootstrap no extent.
        self.assertEqual(rows[0x8000], dict(file=ARM64.name, page_offset=0x8000, macho_segments=['__LINKEDIT'],
                                            macho_sections=[], macho_symbols=[]))

    def test_macho_symbols_end_at_the_next_symbol_or_section_end(self):
        rows = pages_at(ARM64, [0x480, 0x500, 0x520], 32)
        self.assertEqual(rows[0x480]['macho_symbols'], ['_pages_first', '_pages_second'])
        self.assertEqual(rows[0x500]['macho_sections'], ['__TEXT,__text', '__TEXT,__const'])
        self.assertEqual(rows[0x500]['macho_symbols'], ['_pages_third', '_pages_table'])
        # _pages_third stops at the end of __text (0x504).
        self.assertEqual(rows[0x520]['macho_symbols'], ['_pages_table'])

    def test_macho_x86_64_slice(self):
        rows = pages_at(X86_64, [0x1000, 0x2000], 4096)
        self.assertEqual(rows[0x1000]['macho_sections'],
                         ['__DATA,__data', '__DATA,__thread_vars', '__DATA,__thread_data'])
        self.assertEqual(rows[0x2000]['macho_segments'], ['__LINKEDIT'])
        self.assertEqual(pages_at(X86_64, [0x470], 16)[0x470]['macho_symbols'],
                         ['_pages_first', '_pages_second'])
        # A 16 KiB host page holds all three 4 KiB-aligned segments.
        row = pages_at(X86_64, [0], 16384)[0]
        self.assertEqual(set(row), MACHO_FIELDS)
        self.assertEqual(row['macho_segments'], ['__TEXT', '__DATA', '__LINKEDIT'])
        self.assertEqual(len(row['macho_symbols']), 7)

    def test_rows_use_only_their_format_field_names(self):
        elf = pages.object_rows(pages.object_layout(ELF), [0x440], 32, 'facade')
        macho = pages.object_rows(pages.object_layout(ARM64), [0x480], 32, 'facade')
        self.assertEqual(set(elf[0]), ELF_FIELDS)
        self.assertEqual(set(macho[0]), MACHO_FIELDS)
        self.assertEqual(pages.text(elf + macho).splitlines(), [
            '{"elf_sections": [".rodata"], "elf_symbols": ["pages_table"], "file": "facade", "page_offset": 1088}',
            '{"file": "facade", "macho_sections": ["__TEXT,__text"], "macho_segments": ["__TEXT"], '
            '"macho_symbols": ["_pages_first", "_pages_second"], "page_offset": 1152}'])

    def test_unsupported_objects_are_refused_with_the_path(self):
        cases = [
            (b'\xca\xfe\xba\xbe' + bytes(28), 'fat Mach-O is not supported'),
            (b'\xbf\xba\xfe\xca' + bytes(28), 'fat Mach-O is not supported'),
            (pages.MH_MAGIC.to_bytes(4, 'little') + bytes(28), 'only 64-bit'),
            (pages.MH_CIGAM_64.to_bytes(4, 'little') + bytes(28), 'only little-endian'),
            (b'SQLite format 3\0', 'unknown object format'),
            (b'', 'unknown object format'),
        ]
        for index, (data, message) in enumerate(cases):
            path = self.root / f'case-{index}'
            path.write_bytes(data)
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, message) as caught:
                    pages.object_layout(path)
                self.assertTrue(str(caught.exception).startswith(f'{path}: '))
        variants = [
            (self.variant(ARM64, [(4, (0x0200000c).to_bytes(4, 'little'))]), 'CPU type 0x200000c'),
            (self.variant(ELF, [(4, b'\x01')]), 'only 64-bit'),
            (self.variant(ELF, [(5, b'\x02')]), 'only little-endian'),
            # e_shnum = 0 with a section header table: extended numbering.
            (self.variant(ELF, [(0x3c, b'\0\0')]), 'extended section or program header'),
            # pages_first, .symtab entry 3, with st_shndx = SHN_XINDEX.
            (self.variant(ELF, [(0x7b8 + 3 * 24 + 6, b'\xff\xff')]), 'extended section indices'),
            (self.variant(ELF, size=100), 'truncated program header at offset 64'),
            (self.variant(ARM64, size=40), 'truncated segment at offset 32'),
        ]
        for path, message in variants:
            with self.subTest(message=message), self.assertRaisesRegex(ValueError, message):
                pages.object_layout(path)

    def test_offsets_past_the_end_are_refused_with_the_path(self):
        cases = [
            # e_shoff = 2**64 - 1, too large for struct.unpack_from's offset.
            (self.variant(ELF, [(0x28, b'\xff' * 8)]),
             'truncated section header at offset 18446744073709551615'),
            # .symtab's sh_offset (section 18 of the table at 0x9c8) = file size.
            (self.variant(ELF, [(0x9c8 + 18 * 64 + 24, (3848).to_bytes(8, 'little'))]),
             r'section \.symtab \[0xf08, 0xff8\) runs past the end of the file'),
        ]
        for path, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, message) as caught:
                    pages.object_layout(path)
                self.assertTrue(str(caught.exception).startswith(f'{path}: '))

    def test_data_files_report_offset_ranges(self):
        data, user = self.root / 'bigram.db', self.root / 'user.db'
        data.write_bytes(bytes(224))
        user.write_bytes(bytes(224))
        read = FakeResidency({
            'M0': {data: [0x01, 0x00, 0x00, 0x00], user: [0x00, 0x01]},
            'I1': {data: [0x01, 0x03, 0x00, 0x81], user: [0x00, 0x01, 0x01, 0x01]},
        })
        rows = [*pages.data_report(data, read('M0', data), read('I1', data), 64, 'data/bigram.db'),
                *pages.data_report(user, read('M0', user), read('I1', user), 64, 'user/user.db')]
        # Pages 1 and 3 of the data file, the last clipped to its 224 bytes; the
        # user file grew from two pages to four, both new pages adjacent.
        self.assertEqual(rows, [dict(file='data/bigram.db', offset_range=[64, 128]),
                                dict(file='data/bigram.db', offset_range=[192, 224]),
                                dict(file='user/user.db', offset_range=[128, 224])])
        self.assertEqual(json.loads(pages.text(rows).splitlines()[1]),
                         dict(file='data/bigram.db', offset_range=[192, 224]))
        # A page at or past the end of a file that shrank is not reported.
        self.assertEqual(pages.data_rows([0, 4096], 4096, 4096, 'user/user.db'),
                         [dict(file='user/user.db', offset_range=[0, 4096])])

    def test_object_report_maps_pages_newly_resident_between_checkpoints(self):
        read = FakeResidency({'M0': {ARM64: [0x01, 0x00, 0x00]}, 'I1': {ARM64: [0x03, 0x01, 0x00]}})
        rows = pages.object_report(ARM64, read('M0', ARM64), read('I1', ARM64), 16384, 'facade')
        self.assertEqual([(row['page_offset'], row['macho_segments']) for row in rows], [(0x4000, ['__DATA'])])
        self.assertEqual(rows[0]['file'], 'facade')

    def test_checkpoint_pairs_are_neighbours_only(self):
        self.assertEqual(pages.checkpoint_pairs(['M0', 'I1']), [('M0', 'I1')])
        self.assertEqual(pages.checkpoint_pairs(['M0', 'M1', 'M2', 'M3']),
                         [('M0', 'M1'), ('M1', 'M2'), ('M2', 'M3')])
        self.assertEqual(pages.checkpoint_pairs(['M0']), [])
        self.assertEqual(pages.checkpoint_pairs([]), [])

    def test_a_page_that_stays_resident_belongs_to_the_interval_that_brought_it(self):
        read = FakeResidency({'M0': {'f': [0, 0, 0]}, 'M1': {'f': [0, 1, 0]},
                              'M2': {'f': [0, 1, 1]}, 'M3': {'f': [0, 1, 1]}})
        found = [pages.newly_resident(read(earlier, 'f'), read(later, 'f'), 4096)
                 for earlier, later in pages.checkpoint_pairs(['M0', 'M1', 'M2', 'M3'])]
        # Page 1 is new at M1 and stays; page 2 is new at M2; nothing is new at M3.
        self.assertEqual(found, [[4096], [8192], []])

    def test_rounds_are_tallied_per_page(self):
        read = FakeResidency({
            'M0': {'a': [0, 0, 0, 0], 'b': [0, 0, 0, 0], 'c': [1, 0, 0, 0]},
            'I1': {'a': [0, 1, 1, 0], 'b': [0, 1, 0, 1], 'c': [1, 1, 0, 0]}})
        rounds = [pages.newly_resident(read('M0', name), read('I1', name), 4096) for name in 'abc']
        self.assertEqual(pages.tally(rounds), {4096: 3, 8192: 1, 12288: 1})
        # A page listed twice in one round still counts once.
        self.assertEqual(pages.tally([[0, 0], [0]]), {0: 2})
        self.assertEqual(pages.tally([]), {})

    def test_object_rows_carry_the_rounds_that_saw_their_page(self):
        rows = pages.object_tally_rows(pages.object_layout(ELF), {0x440: 3, 0x540: 1}, 32, 'facade')
        self.assertEqual(rows, [
            dict(file='facade', page_offset=0x440, elf_sections=['.rodata'], elf_symbols=['pages_table'], in_rounds=3),
            dict(file='facade', page_offset=0x540, elf_sections=['.text'],
                 elf_symbols=['pages_first', 'pages_second', 'pages_third'], in_rounds=1)])
        self.assertEqual(pages.object_tally_rows(pages.object_layout(ELF), {}, 32, 'facade'), [])

    def test_data_rows_join_pages_only_when_their_round_counts_are_equal(self):
        counts = {0: 3, 64: 3, 128: 2, 192: 3}
        self.assertEqual(pages.data_tally_rows(counts, 64, 224, 'user/user.db'), [
            dict(file='user/user.db', offset_range=[0, 128], in_rounds=3),
            dict(file='user/user.db', offset_range=[128, 192], in_rounds=2),
            dict(file='user/user.db', offset_range=[192, 224], in_rounds=3)])
        self.assertEqual(pages.data_tally_rows({}, 64, 224, 'user/user.db'), [])

    def test_limits_name_readahead_the_checkpoint_pause_and_written_pages(self):
        text = ' '.join(pages.LIMITS)
        for phrase in ('readahead', 'not necessarily a faulted page', 'pausing at a checkpoint', 'finish',
                       'process wrote'):
            self.assertIn(phrase, text)


if __name__ == '__main__':
    unittest.main()
