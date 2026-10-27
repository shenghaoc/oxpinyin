"""processor-trace: the xctrace intervals reader and its order-file comparison."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import processor_trace, recipes
from tools.packaging import function_order

LIBRARY = 'libpinyin.15.0.0.dylib'
# Three calls in the library (two of them the same function), one in libSystem; values are
# written once with an id and referred to by ref, as xctrace exports them.
INTERVALS = f'''<?xml version="1.0"?>
<trace-query-result>
<node xpath='//trace-toc[1]/run[1]/data[1]/table[21]'><schema name="processor-trace-intervals"><col><mnemonic>start</mnemonic></col></schema>
<row><start-time id="1" fmt="00:00.000.000">0</start-time><duration id="2" fmt="5">5</duration><process id="3" fmt="startup (1)"><pid id="4" fmt="1">1</pid></process><thread id="5" fmt="t"><tid id="6" fmt="1">1</tid><process ref="3"/></thread><layout-id id="7" fmt="1">1</layout-id><text-location id="8" fmt="_pinyin_init  {LIBRARY}"><uuid id="9" fmt="U">U</uuid><mach-o-vm-offset id="10" fmt="0x104">260</mach-o-vm-offset></text-location><event-concept id="11" fmt="c">c</event-concept><uint64 id="12" fmt="7">7</uint64><uint64 id="13" fmt="9">9</uint64><sentinel/><text-backtrace id="14"><frame id="15" name="x" addr="0x1"><binary id="16" name="{LIBRARY}" UUID="U"/></frame></text-backtrace></row>
<row><start-time ref="1"/><duration ref="2"/><process ref="3"/><thread ref="5"/><layout-id ref="7"/><text-location ref="8"/><event-concept ref="11"/><uint64 ref="12"/><uint64 id="17" fmt="1">1</uint64><sentinel/><text-backtrace ref="14"/></row>
<row><start-time ref="1"/><duration ref="2"/><process ref="3"/><thread ref="5"/><layout-id ref="7"/><text-location id="18" fmt="0x2000  libSystem.B.dylib"><uuid id="19" fmt="V">V</uuid><mach-o-vm-offset id="20" fmt="0x2000">8192</mach-o-vm-offset></text-location><event-concept ref="11"/><uint64 id="21" fmt="100">100</uint64><uint64 id="22" fmt="200">200</uint64><sentinel/><text-backtrace ref="14"/></row>
<row><start-time ref="1"/><duration ref="2"/><process ref="3"/><thread ref="5"/><layout-id ref="7"/><text-location id="23" fmt="pinyin::open  {LIBRARY}"><uuid ref="9"/><mach-o-vm-offset id="24" fmt="0x120">288</mach-o-vm-offset></text-location><event-concept ref="11"/><uint64 id="25" fmt="3">3</uint64><uint64 id="26" fmt="4">4</uint64><sentinel/><text-backtrace ref="14"/></row>
<row><start-time ref="1"/><duration ref="2"/><process ref="3"/><thread ref="5"/><layout-id ref="7"/><text-location id="27" fmt="0x10  {LIBRARY}"><uuid ref="9"/><mach-o-vm-offset id="28" fmt="0x10">16</mach-o-vm-offset></text-location><event-concept ref="11"/><uint64 id="29" fmt="2">2</uint64><uint64 id="30" fmt="2">2</uint64><sentinel/><text-backtrace ref="14"/></row>
</node>
</trace-query-result>
'''
DARWIN = '''
0000000000000100 (__TEXT,__text) external _pinyin_init
0000000000000120 (__TEXT,__text) non-external __RNvCs1w45a2lSNJp_6pinyin4open
0000000000000140 (__TEXT,__text) non-external __RNvCs1w45a2lSNJp_6pinyin5never
0000000000000160 (__TEXT,__text) non-external __RNvCsBBBB_5crate4keep
0000000000000180 (__TEXT,__text) non-external __RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin
0000000000000020 (__TEXT,__const) non-external table
'''
BSD = '''
0000000000000100 T _pinyin_init
0000000000000120 t pinyin::open
0000000000000140 t pinyin::never
0000000000000160 t crate::keep
0000000000000180 t oxpinyin_core::insert::<&str>
0000000000000020 t table
'''
POLICY = 'oxpinyin-function-order-v1\tlibpinyin.so.15\ninit\tpinyin_init\ninit\tpinyin::never\nsteady\tpinyin::open\nsteady\tabsent::path\n'


class IntervalsTests(unittest.TestCase):
    def test_values_are_aggregated_per_binary_and_offset_through_refs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'intervals.xml'
            path.write_text(INTERVALS)
            totals = processor_trace.intervals(path)
        self.assertEqual(totals, {(LIBRARY, 260): [2, 14, 10], ('libSystem.B.dylib', 8192): [1, 100, 200],
                                  (LIBRARY, 288): [1, 3, 4], (LIBRARY, 16): [1, 2, 2]})
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'empty.xml'
            path.write_text('<trace-query-result><node></node></trace-query-result>')
            with self.assertRaisesRegex(ValueError, 'no processor-trace-intervals rows'):
                processor_trace.intervals(path)

    def test_offsets_map_to_the_containing_text_symbol(self):
        symbols = processor_trace.text_symbols(DARWIN)
        self.assertEqual([raw for _, raw in symbols],
                         ['_pinyin_init', '__RNvCs1w45a2lSNJp_6pinyin4open', '__RNvCs1w45a2lSNJp_6pinyin5never',
                          '__RNvCsBBBB_5crate4keep', '__RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin'])
        totals = {(LIBRARY, 260): [2, 14, 10], ('libSystem.B.dylib', 8192): [1, 100, 200],
                  (LIBRARY, 288): [1, 3, 4], (LIBRARY, 16): [1, 2, 2]}
        executed, unmapped = processor_trace.attribute(totals, symbols, LIBRARY)
        self.assertEqual(executed, {'_pinyin_init': [2, 14, 10], '__RNvCs1w45a2lSNJp_6pinyin4open': [1, 3, 4]})
        self.assertEqual(unmapped, 1)

    def test_the_report_sets_executed_against_the_order_file_by_tier(self):
        symbols = function_order.macho_symbols(DARWIN, BSD)
        executed = {'_pinyin_init': [2, 14, 10], '__RNvCs1w45a2lSNJp_6pinyin4open': [1, 3, 4],
                    '__RNvCsBBBB_5crate4keep': [1, 1, 1],
                    '__RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin': [1, 5, 5]}
        rows = [('init', 'pinyin_init'), ('init', 'pinyin::never'), ('steady', 'pinyin::open'), ('steady', 'absent::path'),
                ('steady', 'oxpinyin_core::insert::<&str> [instantiating crate: oxpinyin_facade]')]
        report = processor_trace.compare(executed, rows, symbols)
        self.assertEqual(report['executed'], dict(functions=4, calls=5, instructions=23, cycles=20))
        self.assertEqual(report['ordered'], dict(functions=3, init=2, steady=1, unresolved=[
            'absent::path', 'oxpinyin_core::insert::<&str> [instantiating crate: oxpinyin_facade]']))
        self.assertEqual(report['executed_ordered'], dict(functions=2, init=1, steady=1, instructions=17))
        self.assertEqual(report['executed_not_ordered'], [
            dict(function='oxpinyin_core::insert::<&str> [instantiating crate: pinyin]',
                 symbol='__RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin', calls=1, instructions=5, cycles=5,
                 policy='other-crate'),
            dict(function='crate::keep', symbol='__RNvCsBBBB_5crate4keep', calls=1, instructions=1, cycles=1, policy='absent')])
        self.assertEqual(report['init_not_executed'], ['pinyin::never'])
        self.assertEqual(report['steady_executed'], ['pinyin::open'])


class RecipeTests(unittest.TestCase):
    def test_the_recipe_is_registered(self):
        self.assertIn('processor-trace', recipes.NAMES)
        self.assertIs(recipes.table()['processor-trace'], processor_trace.processor_trace)

    def test_the_recipe_reads_an_export_and_prints_one_record(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            (work / 'intervals.xml').write_text(INTERVALS)
            (work / 'darwin').write_text(DARWIN)
            (work / 'bsd').write_text(BSD)
            (work / 'policy').write_text(POLICY)
            (work / LIBRARY).write_bytes(b'synthetic dylib')
            (work / 'nm').write_text('#!/bin/sh\ncase " $* " in *" --format=darwin "*) cat "$TEST_NM_DARWIN";; *) cat "$TEST_NM_BSD";; esac\n')
            (work / 'nm').chmod(0o755)
            env = {**os.environ, 'PATH': str(work) + os.pathsep + os.environ['PATH'],
                   'TEST_NM_DARWIN': str(work / 'darwin'), 'TEST_NM_BSD': str(work / 'bsd')}
            command = [sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'measure', 'processor-trace',
                       '--library', str(work / LIBRARY), '--policy', str(work / 'policy')]
            result = subprocess.run([*command, '--intervals', str(work / 'intervals.xml')],
                                    capture_output=True, text=True, env=env, cwd=ROOT)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)['processor-trace']
            self.assertEqual((report['library'], report['binaries'], report['unmapped_offsets']), (LIBRARY, 2, 1))
            self.assertEqual(report['executed']['functions'], 2)
            self.assertEqual(report['init_not_executed'], ['pinyin::never'])
            for arguments in ([], ['--intervals', str(work / 'intervals.xml'), '--trace', 'x.trace']):
                self.assertEqual(subprocess.run([*command, *arguments], capture_output=True, env=env, cwd=ROOT).returncode, 2)
            missing = subprocess.run([*command, '--intervals', str(work / 'absent.xml')], capture_output=True, text=True, env=env, cwd=ROOT)
            self.assertEqual(missing.returncode, 1)
            self.assertIn('missing input', missing.stderr)
            # --trace exports through xcrun xctrace into --export-dir, which need not exist yet.
            (work / 'xcrun').write_text('#!/bin/sh\nfor arg; do if [ "$previous" = --output ]; then cp "$TEST_INTERVALS" "$arg"; fi; previous=$arg; done\n')
            (work / 'xcrun').chmod(0o755)
            (work / 'init.trace').mkdir()
            traced = subprocess.run([*command, '--trace', str(work / 'init.trace'), '--export-dir', str(work / 'kept/export')],
                                    capture_output=True, text=True, env={**env, 'TEST_INTERVALS': str(work / 'intervals.xml')}, cwd=ROOT)
            self.assertEqual(traced.returncode, 0, traced.stderr)
            self.assertEqual(json.loads(traced.stdout), report and json.loads(result.stdout))
            self.assertTrue((work / 'kept/export/processor-trace-intervals.xml').is_file())


if __name__ == '__main__':
    unittest.main()
