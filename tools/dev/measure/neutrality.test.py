#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Tests for neutrality.py: the instruction normalization, symbol accounting and report of the debug-info comparison.

The logic runs on listings written out here and on stand-ins for the GNU tools, so it runs on macOS and Linux.
The last class compiles shared objects and runs the real tools; it is defined on Linux only.
"""
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
        self.out = io.TextIOWrapper(self.out_bytes, encoding='utf-8', newline='\n', write_through=True)
        self.err = io.TextIOWrapper(self.err_bytes, encoding='utf-8', newline='\n', write_through=True)
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
    def test_bytes_round_trip_through_the_text(self):
        data = bytes(range(256)).replace(b'\n', b'')
        self.assertEqual(next(neutrality.lines(data)).encode('latin-1'), data)


class InstructionTests(unittest.TestCase):
    def test_a_comment_ends_the_instruction_and_a_rip_relative_operand_is_masked(self):
        self.assertEqual(texts('lea    0x2ee0(%rip),%rax' + SPACES + '# 4020 <x>'),
                         ['lea    RIPREL,%rax' + ' ' * 7])

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

    def test_aarch64_branches_registers_and_pc_relative_addresses(self):
        self.assertEqual(texts('b\t1040 <f>', 'b.ne\t1050 <g>', 'bl\t1060 <h>', 'br\tx17', 'blr\tx1',
                               'cbz\tx1, 1070 <i>', 'cbnz\tw2, 1080 <j>', 'tbz\tw0, #3, 1090 <k>',
                               'tbnz\tx9, #63, 10a0 <l>', 'adrp\tx16, 1f000 <m>', 'adr\tx0, 10b0 <n>'),
                         ['b TARGET', 'b.ne TARGET', 'bl TARGET', 'br\tx17', 'blr\tx1',
                          'cbz x1, TARGET', 'cbnz w2, TARGET', 'tbz w0, TARGET', 'tbnz x9, TARGET',
                          'adrp x16, PCREL', 'adr x0, PCREL'])

    def test_the_crate_hash_of_a_v0_symbol_is_replaced_elsewhere(self):
        self.assertEqual(texts('mov    $_RNvCsabc123_5crate3foo,%eax',
                               'mov    $_RNvCsabc123_5crate3foo,$_RNvCsdef9_5crate3bar',
                               'mov    $Csabc,%eax'),
                         ['mov    $_RNvCsH_5crate3foo,%eax', 'mov    $_RNvCsH_5crate3foo,$_RNvCsH_5crate3bar',
                          'mov    $Csabc,%eax'])

    def test_the_listing_around_the_code_is_skipped(self):
        data = (b'\nx.so:     file format elf64-x86-64\n\n\nDisassembly of section .init:\n\n'
                b'0000000000001000 <f>:\n  1000:\tnop\n\nsome file format line\n')
        self.assertEqual(list(neutrality.instruction_rows(data)), [('f', 'nop')])


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

    def test_a_failing_objcopy_ends_the_comparison_silently_with_its_status(self):
        for failing in ({'objcopy': 1}, {'objcopy': 3}):
            with self.subTest(failing=failing):
                tools = self.tools((b'x', b''), (b'y', b''), failing=failing)
                self.assertEqual(self.compare(tools), (list(failing.values())[0], ''))
                self.assertEqual(tools.names(), ['objcopy'])
        self.assertEqual(self.streams.stderr, '')

    def test_a_signal_is_reported_as_the_shell_would(self):
        tools = self.tools((b'x', b''), (b'y', b''), failing={'objcopy': -9})
        self.assertEqual(self.compare(tools)[0], 137)

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

    def test_the_scratch_directory_is_gone_after_a_failure(self):
        tools = self.tools((b'x', b''), (b'y', b''), failing={'objdump': 1})
        self.compare(tools)
        for destination in tools.destinations:
            self.assertFalse(destination.parent.exists())


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
                with Streams() as streams:
                    status = callgrind_control(list(map(str, stages)), os.environ)
                self.assertEqual(status, 0, streams.stdout)
                self.assertIn('PASS (a): identical .text', streams.stdout)
                self.assertRegex(streams.stdout, r'profiled DWARF compilation units: [1-9][0-9]*')
                for stage, original in zip(stages, originals):
                    self.assertEqual((stage / 'libpinyin.so').read_bytes(), original)
                    self.assertTrue((stage / 'libpinyin.so').is_symlink())

        def test_other_code_is_case_c(self):
            status, report = self.compare('base', 'different')
            self.assertEqual(status, 1, report)
            self.assertIn('FAIL (c): instruction multiset differs — this is different code.\n', report)
            self.assertTrue(any(row.startswith(('<', '>')) for row in report.splitlines()), report)
            self.assertNotIn('Stop and report', report)


if __name__ == '__main__':
    unittest.main()
