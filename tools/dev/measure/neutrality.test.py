#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for neutrality.py: the instruction normalization, symbol accounting and report of the debug-info comparison.

The logic runs on listings written out here and on stand-ins for the GNU tools, so it runs on macOS and Linux.
The last class compiles shared objects and runs the real tools; it needs gcc and GNU binutils on Linux.
"""
import collections
import contextlib
import hashlib
import io
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

SPACES = ' ' * 8


def listing(*functions):
    """An `objdump -d --no-show-raw-insn` listing of FUNCTIONS, each (name, [instruction text, ...])."""
    rows = ['', 'x.so:     file format elf64-x86-64', '', '', 'Disassembly of section .text:', '']
    address = 0x1000
    for name, body in functions:
        rows.append(f'{address:016x} <{name}>:')
        for text in body:
            rows.append(f'    {address:x}:\t{text}')
            address += 4
        rows.append('')
    return ('\n'.join(rows) + '\n').encode()


def rows(*instructions, function='f'):
    """The normalized rows of a single function's INSTRUCTIONS."""
    return list(neutrality.instruction_rows(listing((function, list(instructions)))))


def texts(*instructions):
    return [text for _, text in rows(*instructions)]


class Streams:
    """Replace the standard streams with byte-backed text wrappers and keep what was written."""

    def __enter__(self):
        self.out_bytes, self.err_bytes = io.BytesIO(), io.BytesIO()
        self.out = io.TextIOWrapper(self.out_bytes, encoding='utf-8', write_through=True)
        self.err = io.TextIOWrapper(self.err_bytes, encoding='utf-8', write_through=True)
        self.stack = contextlib.ExitStack()
        self.stack.enter_context(mock.patch.object(sys, 'stdout', self.out))
        self.stack.enter_context(mock.patch.object(sys, 'stderr', self.err))
        return self

    def __exit__(self, *exc):
        self.stack.close()

    @property
    def stdout(self):
        return self.out_bytes.getvalue().decode()

    @property
    def stderr(self):
        return self.err_bytes.getvalue().decode()


class Tools:
    """Stand-ins for objcopy, objdump, nm and diff: canned output per object, and a record of the calls."""

    def __init__(self, text, listings, symbols=None, diff=b'', failing=(), missing=(), stderr=None):
        self.text, self.listings, self.symbols, self.diff = text, listings, symbols or {}, diff
        self.failing, self.missing, self.stderr = dict(failing), set(missing), stderr or {}
        self.calls, self.histograms, self.destinations = [], [], []

    def __call__(self, argv, **options):
        self.calls.append(list(argv))
        tool = argv[0]
        if tool in self.missing:
            raise FileNotFoundError(tool)
        status = self.failing.get(tool, 0)
        error = self.stderr.get(tool, b'')
        if tool == 'objcopy':
            self.destinations.append(Path(argv[-1]))
            Path(argv[-1]).write_bytes(self.text[argv[-2]])
            return subprocess.CompletedProcess(argv, status, b'', error)
        if tool == 'objdump':
            return subprocess.CompletedProcess(argv, status, self.listings[argv[-1]], error)
        if tool == 'nm':
            return subprocess.CompletedProcess(argv, status, self.symbols[argv[-1]], error)
        if tool == 'diff':
            self.histograms = [Path(path).read_bytes() for path in argv[1:]]
            return subprocess.CompletedProcess(argv, 1, self.diff, error)
        raise AssertionError(f'unexpected tool {tool}')

    def names(self):
        return [call[0] for call in self.calls]


class LineTests(unittest.TestCase):
    def test_records_end_at_newlines_only(self):
        # \r, \v, \f, the 0x1c-0x1e separators and 0x85 all end a line in str.splitlines; awk reads none of them as one.
        data = b'a\rb\x0bc\x0cd\x1ce\x1df\x1eg\x85h\nnext\n'
        self.assertEqual(list(neutrality.lines(data)), ['a\rb\x0bc\x0cd\x1ce\x1df\x1eg\x85h', 'next'])

    def test_a_last_line_without_a_newline_counts(self):
        self.assertEqual(list(neutrality.lines(b'one\ntwo')), ['one', 'two'])
        self.assertEqual(list(neutrality.lines(b'')), [])
        self.assertEqual(list(neutrality.lines(b'\n\n')), ['', ''])

    def test_bytes_round_trip_through_the_text(self):
        data = bytes(range(256)).replace(b'\n', b'')
        self.assertEqual(next(neutrality.lines(data)).encode('latin-1'), data)


class InstructionTests(unittest.TestCase):
    def test_a_comment_ends_the_instruction_and_a_rip_relative_operand_is_masked(self):
        self.assertEqual(texts('lea    0x2ee0(%rip),%rax' + SPACES + '# 4020 <x>'),
                         ['lea    RIPREL,%rax' + ' ' * 7])

    def test_negative_and_zero_displacements_are_masked_too(self):
        self.assertEqual(texts('mov    -0x10(%rip),%rdi', 'mov    0x0(%rip),%rsi'),
                         ['mov    RIPREL,%rdi', 'mov    RIPREL,%rsi'])

    def test_other_displacements_stay(self):
        self.assertEqual(texts('mov    0x10(%rax),%rdi', 'mov    0x10(%rip,%rax),%rsi'),
                         ['mov    0x10(%rax),%rdi', 'mov    0x10(%rip,%rax),%rsi'])

    def test_direct_transfers_keep_only_their_mnemonic(self):
        self.assertEqual(texts('jmp    1040 <puts@plt>', 'je     1050 <f+0x10>', 'jne    1060 <g>',
                               'jrcxz  1070 <h>', 'loop   1080 <i>', 'loopne 1090 <j>', 'call   10a0 <k>'),
                         ['jmp TARGET', 'je TARGET', 'jne TARGET', 'jrcxz TARGET', 'loop TARGET',
                          'loopne TARGET', 'call TARGET'])

    def test_indirect_transfers_stay_whole(self):
        self.assertEqual(texts('jmp    *%rax', 'call   *0x10(%rax)', 'call   *0x2fe2(%rip)'),
                         ['jmp    *%rax', 'call   *0x10(%rax)', 'call   *RIPREL'])

    def test_only_the_exact_call_mnemonic_is_a_call(self):
        # An older binutils prints callq and a prefix leads with bnd; neither is a direct transfer here.
        self.assertEqual(texts('callq  1040 <puts@plt>', 'bnd jmp 1040 <puts@plt>'),
                         ['callq  1040 <puts@plt>', 'bnd jmp 1040 <puts@plt>'])

    def test_aarch64_branches_registers_and_pc_relative_addresses(self):
        self.assertEqual(texts('b\t1040 <f>', 'b.ne\t1050 <g>', 'bl\t1060 <h>', 'br\tx17', 'blr\tx1',
                               'cbz\tx1, 1070 <i>', 'cbnz\tw2, 1080 <j>', 'tbz\tw0, #3, 1090 <k>',
                               'tbnz\tx9, #63, 10a0 <l>', 'adrp\tx16, 1f000 <m>', 'adr\tx0, 10b0 <n>'),
                         ['b TARGET', 'b.ne TARGET', 'bl TARGET', 'br\tx17', 'blr\tx1',
                          'cbz x1, TARGET', 'cbnz w2, TARGET', 'tbz w0, TARGET', 'tbnz x9, TARGET',
                          'adrp x16, PCREL', 'adr x0, PCREL'])

    def test_an_aarch64_immediate_ends_the_instruction_as_a_comment_does(self):
        # The comment rule has no notion of the architecture: " #0x10" is dropped from every aarch64
        # immediate, so a change of immediate alone does not change the multiset there.
        self.assertEqual(texts('add\tx0, x0, #0x10', 'add\tx0, x0, #0x20', 'ldr\tx17, [x16, #4088]'),
                         ['add\tx0, x0,', 'add\tx0, x0,', 'ldr\tx17, [x16,'])

    def test_the_crate_hash_of_a_v0_symbol_is_replaced_elsewhere(self):
        self.assertEqual(texts('mov    $_RNvCsabc123_5crate3foo,%eax',
                               'mov    $_RNvCsabc123_5crate3foo,$_RNvCsdef9_5crate3bar',
                               'mov    $Csabc,%eax'),
                         ['mov    $_RNvCsH_5crate3foo,%eax', 'mov    $_RNvCsH_5crate3foo,$_RNvCsH_5crate3bar',
                          'mov    $Csabc,%eax'])

    def test_a_header_names_the_function_that_follows(self):
        data = (b'0000000000001000 <first>:\n  1000:\tnop\n'
                b'   0000000000002000 <_RNvCsabc_3foo>:\n  2000:\tret\n'
                b'0000000000003000 <a>b>:\n  3000:\thlt\n'
                b'0000000000004000 <.text>:\n  4000:\txor    %eax,%eax\n')
        self.assertEqual(list(neutrality.instruction_rows(data)),
                         [('first', 'nop'), ('_RNvCsabc_3foo', 'ret'), ('a>b', 'hlt'), ('.text', 'xor    %eax,%eax')])

    def test_instructions_before_a_header_belong_to_no_function(self):
        self.assertEqual(list(neutrality.instruction_rows(b'  1000:\tnop\n0000000000001000 <f>:\n  1000:\tret\n')),
                         [('', 'nop'), ('f', 'ret')])

    def test_the_listing_around_the_code_is_skipped(self):
        data = (b'\nx.so:     file format elf64-x86-64\n\n\nDisassembly of section .init:\n\n'
                b'0000000000001000 <f>:\n  1000:\tnop\n\nsome file format line\n')
        self.assertEqual(list(neutrality.instruction_rows(data)), [('f', 'nop')])

    def test_a_line_without_an_address_is_taken_as_it_stands(self):
        self.assertEqual(rows('...'), [('f', '...')])
        self.assertEqual(list(neutrality.instruction_rows(b'0000000000001000 <f>:\n\t...\n')), [('f', '\t...')])

    def test_an_uppercase_address_is_not_a_header(self):
        self.assertEqual(list(neutrality.instruction_rows(b'0000ABCD <f>:\n')), [('', '0000ABCD <f>:')])

    def test_a_trailing_carriage_return_stays_in_the_text(self):
        self.assertEqual(texts('nop\r'), ['nop\r'])


class HistogramTests(unittest.TestCase):
    def test_counts_descend_and_ties_fall_in_descending_byte_order(self):
        census = collections.Counter({'ret': 3, 'nop': 3, 'mov': 1, 'a b': 12, 'add': 3})
        self.assertEqual(neutrality.histogram(census),
                         ['     12 a b', '      3 ret', '      3 nop', '      3 add', '      1 mov'])

    def test_the_count_has_seven_columns_and_never_truncates(self):
        self.assertEqual(neutrality.histogram(collections.Counter({'x': 12345678, 'y': 7})),
                         ['12345678 x', '      7 y'])

    def test_no_instructions_no_rows(self):
        self.assertEqual(neutrality.histogram(collections.Counter()), [])

    def test_a_form_with_a_tab_orders_by_its_bytes(self):
        census = collections.Counter({'add\tx0': 2, 'add x0': 2})
        self.assertEqual(neutrality.histogram(census), ['      2 add x0', '      2 add\tx0'])


class SymbolTests(unittest.TestCase):
    NM = (b'0000000000001130 T foo\n0000000000001140 t bar\n0000000000002000 d data\n'
          b'0000000000001150 W weak\n0000000000001160 T _RNvCsabc123_5crate3foo\n0000000000001170 T foo\n'
          b'0000000000001130 T foo\n0000000000001180 T\n')

    def test_text_symbols_are_unique_hash_stripped_and_sorted_by_byte(self):
        self.assertEqual(neutrality.text_symbols(self.NM),
                         [' 0000000000001180', '_RNvCsH_5crate3foo 0000000000001160', 'bar 0000000000001140',
                          'foo 0000000000001130', 'foo 0000000000001170'])

    def test_two_hashes_of_one_name_and_address_collapse(self):
        nm = b'0000000000001160 T _RNvCsabc_3foo\n0000000000001160 T _RNvCsdef_3foo\n'
        self.assertEqual(neutrality.text_symbols(nm), ['_RNvCsH_3foo 0000000000001160'])

    def test_the_type_decides_not_the_name(self):
        self.assertEqual(neutrality.text_symbols(b'0000000000001000 d t\n0000000000001001 T d\n'),
                         ['d 0000000000001001'])

    def test_addresses_group_by_name(self):
        self.assertEqual(neutrality.addresses(['a 1', 'a 2', 'b 3']), {'a': ['1', '2'], 'b': ['3']})

    def test_unpaired_names_count_each_extra_occurrence(self):
        first = neutrality.addresses(['a 1', 'a 2', 'b 3', 'c 4', 'x 1', 'x 2', 'x 3'])
        second = neutrality.addresses(['a 1', 'a 5', 'c 9', 'd 7', 'x 1'])
        self.assertEqual(neutrality.unpaired(first, second), ['b', 'x', 'x'])
        self.assertEqual(neutrality.unpaired(second, first), ['d'])

    def test_the_empty_name_is_not_counted(self):
        self.assertEqual(neutrality.unpaired(neutrality.addresses([' 1', 'a 2']), {}), ['a'])

    def test_pairing_is_a_join_on_the_name(self):
        first = neutrality.addresses(['a 1', 'a 2', 'b 3', 'c 4'])
        second = neutrality.addresses(['a 1', 'a 5', 'c 9', 'd 7'])
        # a pairs 2 x 2 = 4 ways and one of them agrees; c pairs once and disagrees.
        self.assertEqual(neutrality.pairing(first, second), (5, 4))

    def test_nothing_in_common_pairs_nothing(self):
        self.assertEqual(neutrality.pairing(neutrality.addresses(['a 1']), neutrality.addresses(['b 1'])), (0, 0))


class DigestTests(unittest.TestCase):
    def test_an_x86_body_is_hashed_whole_with_a_separator_after_each_instruction(self):
        expected = hashlib.sha256(b'mov    %rax,%rbx\x1cret\x1c').hexdigest()
        data = listing(('f', ['mov    %rax,%rbx', 'ret']))
        self.assertEqual(neutrality.body_digests(data), [('f', expected)])

    def test_an_aarch64_body_is_hashed_by_its_mnemonics_alone(self):
        # objdump separates mnemonic and operands with a tab there, and only the first tab-delimited chunk counts.
        expected = hashlib.sha256(b'add\x1cret\x1c').hexdigest()
        data = listing(('f', ['add\tx0, x0, x1', 'ret']))
        self.assertEqual(neutrality.body_digests(data), [('f', expected)])
        other = listing(('f', ['add\tx5, x6, x7', 'ret']))
        self.assertEqual(neutrality.body_digests(other), [('f', expected)])

    def test_a_name_that_heads_two_blocks_hashes_them_as_one_body(self):
        data = listing(('f', ['nop']), ('g', ['ret']), ('f', ['ret']))
        digests = dict(neutrality.body_digests(data))
        self.assertEqual(digests['f'], hashlib.sha256(b'nop\x1cret\x1c').hexdigest())
        self.assertEqual(digests['g'], hashlib.sha256(b'ret\x1c').hexdigest())

    def test_names_lose_their_crate_hash_and_instructions_without_a_function_are_left_out(self):
        data = b'  1000:\tnop\n0000000000001000 <_RNvCsabc_3foo>:\n  1000:\tret\n'
        self.assertEqual(neutrality.body_digests(data), [('_RNvCsH_3foo', hashlib.sha256(b'ret\x1c').hexdigest())])

    def test_the_padding_row_adds_only_its_separator(self):
        data = b'0000000000001000 <f>:\n\t...\n  1000:\tret\n'
        self.assertEqual(neutrality.body_digests(data), [('f', hashlib.sha256(b'\x1cret\x1c').hexdigest())])


class NoteTests(unittest.TestCase):
    def note(self, first_only, second_only, first, second):
        return neutrality.leftover_note(first_only, second_only, lambda: first, lambda: second)

    def test_no_leftovers(self):
        self.assertEqual(self.note([], [], [], []), 'none')
        # A leftover on the second side alone is not counted: only the first side's count is.
        self.assertEqual(self.note([], ['b'], [], []), 'none')

    def test_unequal_counts_cannot_be_paired(self):
        self.assertEqual(self.note(['a', 'b'], ['c'], [], []), 'asymmetric leftovers (2 vs 1) — not pairable')

    def test_equal_bodies_pair(self):
        self.assertEqual(self.note(['a'], ['b'], [('a', 'h1'), ('x', 'h2')], [('b', 'h1'), ('y', 'h3')]),
                         '1 paired by identical instruction body')

    def test_digests_pair_as_multisets(self):
        # Two leftover names with the same body need two matches on the other side.
        self.assertEqual(self.note(['a', 'b'], ['c', 'd'], [('a', 'h'), ('b', 'h')], [('c', 'h'), ('d', 'k')]),
                         'only 1 of 2 paired by instruction body')

    def test_nothing_matching(self):
        self.assertEqual(self.note(['a'], ['b'], [('a', 'h1')], [('b', 'h2')]),
                         'only 0 of 1 paired by instruction body')

    def test_a_name_that_several_functions_share_can_match_more_than_it_has_leftovers(self):
        self.assertEqual(self.note(['a'], ['b'], [('a', 'h1'), ('a', 'h2')], [('b', 'h1'), ('b', 'h2')]),
                         'only 2 of 1 paired by instruction body')


class CompareTests(unittest.TestCase):
    def setUp(self):
        self.streams = self.enterContext(Streams())

    def compare(self, tools, first='a.so', second='b.so'):
        return neutrality.compare(first, second, run=tools)

    def tools(self, first, second, symbols=None, **options):
        """Tools serving a.so and b.so: FIRST and SECOND are (.text bytes, listing bytes)."""
        return Tools({'a.so': first[0], 'b.so': second[0]}, {'a.so': first[1], 'b.so': second[1]},
                     symbols and {'a.so': symbols[0], 'b.so': symbols[1]}, **options)

    def test_identical_text_is_case_a_and_asks_for_nothing_more(self):
        tools = self.tools((b'same', b''), (b'same', b''))
        status, report = self.compare(tools)
        digest = hashlib.sha256(b'same').hexdigest()
        self.assertEqual((status, report),
                         (0, f'with debug info:    .text sha256 {digest} (4 bytes)\n'
                             f'without debug info: .text sha256 {digest} (4 bytes)\n'
                             'PASS (a): identical .text — debug info changed nothing else\n'))
        self.assertEqual(tools.names(), ['objcopy', 'objcopy'])
        self.assertEqual(tools.calls[0][:4], ['objcopy', '-O', 'binary', '--only-section=.text'])
        self.assertEqual(tools.calls[0][4], 'a.so')

    def test_a_pair_of_paths_is_accepted_as_well_as_strings(self):
        tools = self.tools((b'same', b''), (b'same', b''))
        self.assertEqual(neutrality.compare(Path('a.so'), Path('b.so'), run=tools)[0], 0)

    def test_the_same_instructions_at_other_addresses_are_case_b(self):
        body = ['mov    %rax,%rbx', 'call   1040 <puts@plt>', 'ret']
        first = listing(('f', body), ('g', ['nop']))
        second = listing(('g', ['nop']), ('f', ['mov    %rax,%rbx', 'call   1090 <puts@plt>', 'ret']))
        symbols = (b'0000000000001000 T f\n0000000000001010 T g\n',
                   b'0000000000001020 T f\n0000000000001000 T g\n')
        tools = self.tools((b'one', first), (b'two', second), symbols)
        status, report = self.compare(tools)
        first_digest, second_digest = hashlib.sha256(b'one').hexdigest(), hashlib.sha256(b'two').hexdigest()
        self.assertEqual(status, 0)
        self.assertEqual(report,
                         f'with debug info:    .text sha256 {first_digest} (3 bytes)\n'
                         f'without debug info: .text sha256 {second_digest} (3 bytes)\n'
                         'instructions (normalized full text): 4 vs 4, 4 distinct forms\n'
                         'PASS (b): identical instruction multiset (full normalized text).\n'
                         '  Symbol accounting: 2 symbols pair by hash-stripped name,\n'
                         '  2 of those at different addresses; unpaired after hash\n'
                         '  strip: 0 — none.\n'
                         '  Ir totals are unaffected by relocation. The SIMULATED cache and\n'
                         '  branch figures are layout-sensitive and must be read with that\n'
                         '  caveat stated; never present them as measurements.\n')
        self.assertEqual(tools.names(), ['objcopy', 'objcopy', 'objdump', 'objdump', 'nm', 'nm'])
        self.assertEqual(tools.calls[2], ['objdump', '-d', '--no-show-raw-insn', 'a.so'])
        self.assertEqual(tools.calls[4], ['nm', '--defined-only', 'a.so'])

    def test_a_renamed_function_with_the_same_body_pairs_by_its_body(self):
        first = listing(('f', ['nop', 'ret']), ('_RNvCsabc_3foo', ['xor    %eax,%eax']))
        second = listing(('h', ['nop', 'ret']), ('_RNvCsdef_3foo', ['xor    %eax,%eax']))
        symbols = (b'0000000000001000 T f\n0000000000001010 T _RNvCsabc_3foo\n',
                   b'0000000000001000 T h\n0000000000001010 T _RNvCsdef_3foo\n')
        status, report = self.compare(self.tools((b'x', first), (b'y', second), symbols))
        self.assertEqual(status, 0)
        self.assertIn('  Symbol accounting: 1 symbols pair by hash-stripped name,\n'
                      '  0 of those at different addresses; unpaired after hash\n'
                      '  strip: 1 — 1 paired by identical instruction body.\n', report)

    def test_bodies_of_the_same_instructions_in_another_order_do_not_pair(self):
        first = listing(('f', ['add    %rax,%rbx', 'ret']))
        second = listing(('g', ['ret', 'add    %rax,%rbx']))
        symbols = (b'0000000000001000 T f\n', b'0000000000001000 T g\n')
        status, report = self.compare(self.tools((b'x', first), (b'y', second), symbols))
        self.assertEqual(status, 0)
        self.assertIn('  Symbol accounting: 0 symbols pair by hash-stripped name,\n', report)
        self.assertIn('  strip: 1 — only 0 of 1 paired by instruction body.\n', report)

    def test_unequal_leftover_counts_are_called_asymmetric(self):
        first = listing(('f1', ['add    %rax,%rbx']), ('f2', ['ret']))
        second = listing(('g', ['add    %rax,%rbx', 'ret']))
        symbols = (b'0000000000001000 T f1\n0000000000001010 T f2\n', b'0000000000001000 T g\n')
        status, report = self.compare(self.tools((b'x', first), (b'y', second), symbols))
        self.assertEqual(status, 0)
        self.assertIn('  strip: 2 — asymmetric leftovers (2 vs 1) — not pairable.\n', report)

    def test_empty_listings_print_empty_totals(self):
        symbols = (b'', b'')
        status, report = self.compare(self.tools((b'x', b''), (b'y', b''), symbols))
        self.assertEqual(status, 0)
        self.assertIn('instructions (normalized full text):  vs , 0 distinct forms\n', report)
        self.assertIn('  Symbol accounting: 0 symbols pair by hash-stripped name,\n', report)

    def test_a_different_instruction_multiset_is_case_c_without_the_closing_lines(self):
        first = listing(('f', ['mov    %rax,%rbx', 'ret']))
        second = listing(('f', ['mov    %rax,%rcx', 'ret']))
        diff = b'1c1\n<       1 mov    %rax,%rbx\n---\n>       1 mov    %rax,%rcx\n'
        tools = self.tools((b'x', first), (b'y', second), diff=diff)
        status, report = self.compare(tools)
        self.assertEqual(status, 1)
        self.assertEqual(report.splitlines()[2:],
                         ['instructions (normalized full text): 2 vs 2, 2 distinct forms',
                          'FAIL (c): instruction multiset differs — this is different code.',
                          '1c1', '<       1 mov    %rax,%rbx', '---', '>       1 mov    %rax,%rcx'])
        # The shell function ended at the failing `diff | head` under set -e and pipefail,
        # before the two lines that say "Stop and report"; they never appeared.
        self.assertNotIn('Stop and report', report)
        self.assertEqual(tools.names(), ['objcopy', 'objcopy', 'objdump', 'objdump', 'diff'])
        self.assertEqual(tools.histograms,
                         [b'      1 ret\n      1 mov    %rax,%rbx\n', b'      1 ret\n      1 mov    %rax,%rcx\n'])

    def test_only_the_first_twenty_lines_of_the_difference_are_shown(self):
        first = listing(('f', ['nop']))
        second = listing(('f', ['ret']))
        diff = ''.join(f'line {number}\n' for number in range(1, 41)).encode()
        status, report = self.compare(self.tools((b'x', first), (b'y', second), diff=diff))
        shown = report.splitlines()[4:]
        self.assertEqual(shown, [f'line {number}' for number in range(1, 21)])

    def test_a_short_difference_adds_no_blank_line(self):
        status, report = self.compare(self.tools((b'x', listing(('f', ['nop']))), (b'y', listing(('f', ['ret']))),
                                                 diff=b'only\n'))
        self.assertTrue(report.endswith('this is different code.\nonly\n'))

    def test_diff_complaints_reach_standard_error(self):
        tools = self.tools((b'x', listing(('f', ['nop']))), (b'y', listing(('f', ['ret']))),
                           stderr={'diff': b'diff: trouble\n'})
        self.compare(tools)
        self.assertEqual(self.streams.stderr, 'diff: trouble\n')

    def test_a_failing_objcopy_ends_the_comparison_silently_with_its_status(self):
        for failing in ({'objcopy': 1}, {'objcopy': 3}):
            with self.subTest(failing=failing):
                tools = self.tools((b'x', b''), (b'y', b''), failing=failing)
                self.assertEqual(self.compare(tools), (list(failing.values())[0], ''))
                self.assertEqual(tools.names(), ['objcopy'])
        self.assertEqual(self.streams.stderr, '')

    def test_objcopy_complaints_stay_hidden(self):
        tools = self.tools((b'x', b''), (b'y', b''), failing={'objcopy': 1}, stderr={'objcopy': b'objcopy: gone\n'})
        self.compare(tools)
        self.assertEqual(self.streams.stderr, '')

    def test_a_failing_objdump_keeps_what_was_printed_before_it(self):
        tools = self.tools((b'x', b''), (b'y', b''), failing={'objdump': 2})
        status, report = self.compare(tools)
        self.assertEqual(status, 2)
        self.assertEqual(len(report.splitlines()), 2)
        self.assertEqual(self.streams.stderr, '')

    def test_a_signal_is_reported_as_the_shell_would(self):
        tools = self.tools((b'x', b''), (b'y', b''), failing={'objcopy': -9})
        self.assertEqual(self.compare(tools)[0], 137)

    def test_a_missing_objcopy_is_status_127_and_silent(self):
        tools = self.tools((b'x', b''), (b'y', b''), missing={'objcopy'})
        self.assertEqual(self.compare(tools), (127, ''))
        self.assertEqual(self.streams.stderr, '')

    def test_a_missing_nm_is_named_and_a_failing_one_shows_its_complaint(self):
        body = listing(('f', ['nop']))
        tools = self.tools((b'x', body), (b'y', body), (b'', b''), missing={'nm'})
        status, report = self.compare(tools)
        self.assertEqual(status, 127)
        self.assertEqual(self.streams.stderr, 'nm: command not found\n')
        self.assertIn('instructions (normalized full text)', report)
        tools = self.tools((b'x', body), (b'y', body), (b'', b''), failing={'nm': 1}, stderr={'nm': b'nm: broken\n'})
        self.assertEqual(self.compare(tools)[0], 1)
        self.assertTrue(self.streams.stderr.endswith('nm: broken\n'))

    def test_nm_complaints_are_shown_even_when_it_succeeds(self):
        body = listing(('f', ['nop']))
        tools = self.tools((b'x', body), (b'y', body), (b'', b''), stderr={'nm': b'nm: no symbols\n'})
        self.assertEqual(self.compare(tools)[0], 0)
        self.assertEqual(self.streams.stderr, 'nm: no symbols\n' * 2)

    def test_the_scratch_directory_is_gone_afterwards(self):
        tools = self.tools((b'same', b''), (b'same', b''))
        self.compare(tools)
        self.assertEqual(len(tools.destinations), 2)
        for destination in tools.destinations:
            self.assertFalse(destination.parent.exists())

    def test_the_scratch_directory_is_gone_after_a_failure(self):
        tools = self.tools((b'x', b''), (b'y', b''), failing={'objdump': 1})
        self.compare(tools)
        for destination in tools.destinations:
            self.assertFalse(destination.parent.exists())


@unittest.skipUnless(sys.platform.startswith('linux') and all(shutil.which(tool) for tool in
                                                              ('gcc', 'objcopy', 'objdump', 'nm', 'diff')),
                     'needs gcc and GNU binutils (objcopy, objdump, nm) on Linux')
class RealToolsTests(unittest.TestCase):
    """Shared objects compiled here, and the real tools: every outcome, on whatever architecture this is."""

    BASE = '''static int table[4] = {1, 2, 3, 4};
int __attribute__((noinline)) f(int x) { return x * 3 + 1; }
int __attribute__((noinline)) g(int y) { return f(y) + 7; }
int _RNvCsabc123_5crate3foo(int z) { return z ^ 0x55; }
int %(name)s(int q) { return q + 40; }
int get(int i) { return table[i & 3]; }
'''
    PAD = 'static char pad[8192] __attribute__((used)) = {1};\n'

    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.directory.cleanup)
        work = Path(cls.directory.name)
        base = cls.BASE % {'name': 'h_same'}
        sources = {'base': base, 'base_g': base,
                   'shifted': cls.PAD + base.replace('abc123', 'def456'),
                   'renamed': cls.PAD + (cls.BASE % {'name': 'h_renamed'}).replace('abc123', 'def456'),
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

    def test_debug_info_alone_leaves_the_text_identical(self):
        status, report = self.compare('base_g', 'base')
        self.assertEqual(status, 0, report)
        self.assertIn('PASS (a): identical .text', report)

    def test_a_shifted_data_object_changes_displacements_only(self):
        status, report = self.compare('base', 'shifted')
        self.assertEqual(status, 0, report)
        if 'PASS (a)' in report:
            self.skipTest('this compiler and linker keep the text identical when data moves')
        self.assertIn('PASS (b): identical instruction multiset', report)
        self.assertRegex(report, r'  Symbol accounting: \d+ symbols pair by hash-stripped name,\n')
        self.assertIn('  strip: 0 — none.\n', report)

    def test_a_renamed_function_pairs_by_its_body(self):
        status, report = self.compare('base', 'renamed')
        self.assertEqual(status, 0, report)
        if 'PASS (a)' in report:
            self.skipTest('this compiler and linker keep the text identical when data moves')
        self.assertIn('  strip: 1 — 1 paired by identical instruction body.\n', report)

    def test_other_code_is_case_c(self):
        status, report = self.compare('base', 'different')
        self.assertEqual(status, 1, report)
        self.assertIn('FAIL (c): instruction multiset differs — this is different code.\n', report)
        self.assertTrue(any(row.startswith(('<', '>')) for row in report.splitlines()), report)
        self.assertNotIn('Stop and report', report)


if __name__ == '__main__':
    unittest.main()
