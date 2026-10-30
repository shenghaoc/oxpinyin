#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the nightly snapshot recipe (tools/dev/measure/snapshot.py).

Every external command and host fact goes through snapshot.Tools, so the order of
the run, each metric rule and the document are tested here on any platform with a
recording fake. One test reads a real shared object with GNU readelf, which needs
a GCC that builds ELF, so it runs on Linux only.
"""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import pool, runners
from tools.dev.measure import bench, pages, recipes, snapshot
from tools.dev.runners import Finished

COMMIT = 'a' * 40
HARNESS_COMMIT = 'b' * 40
NOW = '2026-10-27T01:02:03Z'

# `readelf -S -W` of a linked cdylib. Single-digit indices print as "[ 4]", which an
# awk field split turned into "[" and "4]"; the Off column differs from Size in every
# wanted row, so a reader that takes the wrong numeric column cannot pass by luck.
READELF = '''There are 14 section headers, starting at offset 0x3e48:

Section Headers:
  [Nr] Name              Type            Address          Off    Size   ES Flg Lk Inf Al
  [ 0]                   NULL            0000000000000000 000000 000000 00      0   0  0
  [ 1] .note.gnu.build-id NOTE            0000000000000238 000238 000024 00   A  0   0  4
  [ 2] .dynsym           DYNSYM          0000000000000260 000260 000108 18   A  7   1  8
  [ 3] .rela.dyn         RELA            0000000000000368 000368 000090 18   A  2   0  8
  [ 4] .text             PROGBITS        0000000000001000 001000 0001c0 00  AX  0   0 16
  [ 5] .rodata           PROGBITS        0000000000002000 002000 000020 00   A  0   0  4
  [ 6] .eh_frame_hdr     PROGBITS        0000000000002020 002020 00001c 00   A  0   0  4
  [ 7] .eh_frame         PROGBITS        0000000000002040 002040 000058 00   A  0   0  8
  [ 8] .data.rel.ro      PROGBITS        0000000000003e00 002e00 000040 00  WA  0   0 32
  [ 9] .data             PROGBITS        0000000000004000 003000 000010 00  WA  0   0  8
  [10] .bss              NOBITS          0000000000004010 003010 000008 00  WA  0   0  1
  [11] .gcc_except_table PROGBITS        0000000000004018 003018 0000a0 00   A  0   0  4
  [12] .symtab           SYMTAB          0000000000000000 0030b8 000150 18     13   9  8
  [13] .strtab           STRTAB          0000000000000000 003208 0000ff 00      0   0  1
Key to Flags:
  W (write), A (alloc), X (execute), M (merge), S (strings), I (info),
'''
# .rela.dyn + .text + .rodata + .eh_frame_hdr + .eh_frame + .data.rel.ro + .gcc_except_table
SECTION_SUM = 0x90 + 0x1c0 + 0x20 + 0x1c + 0x58 + 0x40 + 0xa0


@contextlib.contextmanager
def captured():
    """Replace stdout and stderr with byte-backed text streams, which runners.write needs.

    Yield the two BytesIO; the wrappers are detached, not closed, so they stay readable.
    """
    out, err = io.BytesIO(), io.BytesIO()
    wrappers = io.TextIOWrapper(out, write_through=True), io.TextIOWrapper(err, write_through=True)
    try:
        with mock.patch.object(sys, 'stdout', wrappers[0]), mock.patch.object(sys, 'stderr', wrappers[1]):
            yield out, err
    finally:
        for wrapper in wrappers:
            wrapper.detach()


def row(**fields):
    """A harness output line."""
    return (json.dumps(fields) + '\n').encode()


def rss_row(first, last):
    """A speed-mode row with the resident set after the first and the last cycle."""
    return row(after_first={'rss_kib': first, 'hwm_kib': first + 1},
               after_last={'rss_kib': last, 'hwm_kib': last + 1})


class FakeTools(snapshot.Tools):
    """Records every command and answers it from a script instead of running it.

    install.sh leaves a shipped library in the stage, strip shrinks it, the
    alloc-count build leaves its cdylib and gcc its harness, so the recipe sees
    the files the real tools would have made.
    """

    def __init__(self, *, present=snapshot.REQUIRED_TOOLS + ('taskset', 'valgrind'), alloc=None,
                 rss=None, readelf=READELF, nm=b'T oxpinyin_init\n', valgrind_status=0, callgrind_ir=1234,
                 answers=None):
        self.present = set(present)
        self.alloc = alloc if alloc is not None else row(steady_alloc_cycles=4, steady_alloc_count=100,
                                                         steady_alloc_bytes=4098)
        self.rss = list(rss if rss is not None else [(0, rss_row(100, 200))] * 5)
        self.readelf, self.nm = readelf, nm
        self.valgrind_status, self.callgrind_ir = valgrind_status, callgrind_ir
        self.answers = {('git', 'rev-parse', 'HEAD'): COMMIT,
                        ('git', 'log', '-1', '--format=%H', '--', 'tools/bisection/bisect.c',
                         'tools/perf-gate'): HARNESS_COMMIT,
                        ('uname', '-srvm'): 'Linux 6.8.0 #1 SMP x86_64', ('rustc', '-V'): 'rustc 1.97.1',
                        ('cc', '--version'): 'cc (Debian 15.2.0) 15.2.0',
                        ('ldd', '--version'): 'ldd (Debian GLIBC 2.42) 2.42',
                        ('valgrind', '--version'): 'valgrind-3.26.0', ('readelf', '--version'): 'GNU readelf 2.45'}
        self.answers.update(answers or {})
        self.events = []
        self.shipped = self.work = None

    def which(self, name):
        self.events.append(('which', name))
        return f'/usr/bin/{name}' if name in self.present else None

    def run(self, command, **options):
        command = [str(part) for part in command]
        self.events.append(('run', command, {'stderr': options.get('stdout') is sys.stderr,
                                              'target': (options.get('env') or {}).get('CARGO_TARGET_DIR')}))
        if command[:len(snapshot.INSTALL)] == snapshot.INSTALL:
            destdir = next(part for part in command if part.startswith('--destdir=')).split('=', 1)[1]
            self.work = Path(destdir).parent
            self.shipped = Path(destdir) / 'usr/lib/libpinyin.so.15.0.0'
            self.shipped.parent.mkdir(parents=True)
            self.shipped.write_bytes(b'shipped library bytes')
        elif command[0] == 'strip':
            Path(command[2]).write_bytes(b'stripped')
        elif command[0] == 'cargo':
            cdylib = Path(options['env']['CARGO_TARGET_DIR']) / 'release/libpinyin_capi.so'
            cdylib.parent.mkdir(parents=True)
            cdylib.write_bytes(b'alloc-count cdylib')
        elif command[0] == 'gcc':
            Path(command[command.index('-o') + 1]).write_bytes(b'harness')
        return 0

    def capture(self, command, **options):
        command = [str(part) for part in command]
        environment = options.get('env') or {}
        perf = {name: environment[name] for name in ('PERF_BACKEND', 'PERF_MODE', 'PERF_CYCLES', 'LD_LIBRARY_PATH')
                if name in environment}
        self.events.append(('capture', command, perf))
        if command[:2] == ['nm', '-D']:
            return pool.Completed(0, self.nm, b'')
        if command[:3] == ['readelf', '-S', '-W']:
            return pool.Completed(0, self.readelf.encode(), b'')
        if '--perf' in command:
            library = command[command.index('--perf') + 1]
            if library.endswith('libpinyin_capi.so'):
                return pool.Completed(0, self.alloc, b'discarded')
            if 'valgrind' in command:
                for part in command:
                    if part.startswith('--callgrind-out-file='):
                        Path(part.split('=', 1)[1]).write_text('callgrind')
                return pool.Completed(self.valgrind_status, b'', b'')
            status, output = self.rss.pop(0)
            return pool.Completed(status, output, b'')
        raise AssertionError('unexpected capture: ' + command[0])

    def query(self, *command):
        self.events.append(('query', list(command)))
        return self.answers.get(command, 'unknown')


    def object_ir(self, path, want):
        self.events.append(('object_ir', str(path), want))
        return self.callgrind_ir

    def now(self):
        return NOW

    def commands(self, kind='run'):
        """The recorded commands of one kind, in order."""
        return [event[1] for event in self.events if event[0] == kind]


class Scratch(unittest.TestCase):
    """A checkout stand-in holding the w3 fixture directories, and a run helper."""

    def setUp(self):
        self.base = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.root = self.base / 'checkout'
        for extension in ('tkt', 'kct'):
            (self.root / 'fixtures/w3' / extension / 'nested').mkdir(parents=True)
            (self.root / 'fixtures/w3' / extension / 'one.bin').write_bytes(extension.encode())
            (self.root / 'fixtures/w3' / extension / 'nested/two.bin').write_bytes(b'two')
        self.out = self.base / 'snapshot.json'
        self.environment = {'PATH': '/usr/bin', 'PERF_CPU': '3', 'GITHUB_EVENT_NAME': 'schedule',
                            'GITHUB_SERVER_URL': 'https://github.com', 'GITHUB_REPOSITORY': 'o/r',
                            'GITHUB_RUN_ID': '42', 'ImageOS': 'ubuntu24', 'ImageVersion': '20261001.1'}

    def run_snapshot(self, argv, tools=None, **environment):
        """Run the recipe under the failure ladder every runner shares; return (status, stdout, stderr)."""
        tools = tools or FakeTools()
        env = dict(self.environment, **environment)
        with captured() as (out, err):
            status = runners.guarded(lambda: snapshot.snapshot(argv, env, tools=tools, root=self.root))
        return status, out.getvalue().decode(), err.getvalue().decode()

    def normal(self, tools, value):
        """Replace the run's private work directory in a recorded value with <work>."""
        return json.loads(json.dumps(value).replace(json.dumps(str(tools.work))[1:-1], '<work>'))


class PureTests(unittest.TestCase):
    def test_section_sum_counts_only_the_wanted_sections_by_their_size_column(self):
        self.assertEqual(snapshot.section_sum(READELF), SECTION_SUM)

    def test_section_sum_reads_single_and_triple_digit_indices(self):
        text = ('  [ 4] .text             PROGBITS        0 001000 000010 00  AX  0   0 16\n'
                '  [100] .rodata          PROGBITS        0 002000 000020 00   A  0   0  4\n'
                '  [9] .eh_frame          PROGBITS        0 002040 000040 00   A  0   0  8\n')
        self.assertEqual(snapshot.section_sum(text), 0x10 + 0x20 + 0x40)

    def test_section_sum_ignores_unwanted_sections_headers_and_empty_input(self):
        self.assertEqual(snapshot.section_sum('  [ 9] .data PROGBITS 0 003000 000010 00 WA 0 0 8\n'), 0)
        self.assertEqual(snapshot.section_sum('Section Headers:\n  [Nr] Name Type Address Off Size\n'), 0)
        self.assertEqual(snapshot.section_sum(''), 0)

    def test_alloc_metrics_divide_by_the_steady_cycles(self):
        self.assertEqual(snapshot.alloc_metrics(json.loads(row(steady_alloc_cycles=4, steady_alloc_count=101,
                                                                steady_alloc_bytes=4099))),
                         {'alloc_count_per_cycle': 25, 'alloc_bytes_per_cycle': 1024})

    def test_alloc_metrics_record_absence_as_none_never_zero(self):
        none = {'alloc_count_per_cycle': None, 'alloc_bytes_per_cycle': None}
        for fields in ({}, {'steady_alloc_cycles': 4}, {'steady_alloc_cycles': 0, 'steady_alloc_count': 5},
                       {'steady_alloc_count': 5, 'steady_alloc_bytes': 9}):
            with self.subTest(fields=fields):
                self.assertEqual(snapshot.alloc_metrics(fields), none)

    def test_alloc_metrics_keep_a_measured_zero(self):
        self.assertEqual(snapshot.alloc_metrics({'steady_alloc_cycles': 4, 'steady_alloc_count': 0,
                                                 'steady_alloc_bytes': 0}),
                         {'alloc_count_per_cycle': 0, 'alloc_bytes_per_cycle': 0})

    def test_rss_metrics_are_integer_medians_of_the_nested_fields(self):
        # Skewed on purpose: a mean would be 400 and 1676.
        rows = [json.loads(rss_row(100, 10)), json.loads(rss_row(1000, 5000)), json.loads(rss_row(101, 20))]
        self.assertEqual(snapshot.rss_metrics(rows), {'rss_first_cycle_kib': 101, 'rss_last_cycle_kib': 20})
        even = [json.loads(rss_row(101, 10)), json.loads(rss_row(200, 21))]
        self.assertEqual(snapshot.rss_metrics(even), {'rss_first_cycle_kib': 150, 'rss_last_cycle_kib': 15})

    def test_rss_metrics_skip_rows_without_a_number_and_record_absence_as_none(self):
        rows = [{'after_first': {'rss_kib': 100}}, {'after_first': 'no'}, {'after_first': {'rss_kib': 'x'}},
                {'after_last': {'hwm_kib': 5}}, {}]
        self.assertEqual(snapshot.rss_metrics(rows), {'rss_first_cycle_kib': 100, 'rss_last_cycle_kib': None})
        self.assertEqual(snapshot.rss_metrics([]), {'rss_first_cycle_kib': None, 'rss_last_cycle_kib': None})

    def test_first_row_reads_one_line_and_refuses_anything_else(self):
        self.assertEqual(snapshot.first_row(b'{"a": 1}\n{"a": 2}\n'), {'a': 1})
        self.assertEqual(snapshot.first_row(b'{"a": 1}'), {'a': 1})
        with self.assertRaises(ValueError):
            snapshot.first_row(b'')
        with self.assertRaises(ValueError):
            snapshot.first_row(b'not json\n')

    def test_fixture_digest_hashes_each_file_name_and_bytes_in_path_order(self):
        with tempfile.TemporaryDirectory() as directory:
            data = Path(directory)
            (data / 'b').mkdir()
            (data / 'b' / 'z.bin').write_bytes(b'zz')
            (data / 'a.bin').write_bytes(b'aa')
            expected = hashlib.sha256(b'a.bin' + b'aa' + b'z.bin' + b'zz').hexdigest()
            self.assertEqual(snapshot.fixture_digest(data), expected)

    def test_a_snapshot_without_provenance_is_refused(self):
        good = {'commit': COMMIT, 'environment': {'harness_commit': HARNESS_COMMIT}}
        snapshot.check_provenance(good)
        for commit, harness, field in (('unknown', HARNESS_COMMIT, 'commit'), (COMMIT, 'unknown', 'harness_commit'),
                                       ('unknown', 'unknown', 'commit')):
            with self.subTest(field=field), captured() as (out, err), self.assertRaises(Finished) as finished:
                snapshot.check_provenance({'commit': commit, 'environment': {'harness_commit': harness}})
            self.assertEqual(finished.exception.status, 1)
            self.assertEqual(err.getvalue().decode(),
                             f'fatal: {field} could not be resolved — a sample with no provenance must not '
                             'enter the series (is this a git checkout, and is safe.directory set?)\n')
            self.assertEqual(out.getvalue(), b'')

    def test_the_real_query_reads_the_first_line_and_says_unknown_otherwise(self):
        tools = snapshot.Tools()
        self.assertEqual(tools.query(sys.executable, '-c', 'print("first\\nsecond")'), 'first')
        self.assertEqual(tools.query(sys.executable, '-c', 'print("  padded  ")'), 'padded')
        self.assertEqual(tools.query(sys.executable, '-c', 'pass'), 'unknown')
        self.assertEqual(tools.query('/nonexistent/oxpinyin-tool'), 'unknown')
        # A failing command's output still counts, as the heredoc's run() never checked a status.
        self.assertEqual(tools.query(sys.executable, '-c', 'import sys; print("said"); sys.exit(3)'), 'said')

    def test_the_real_object_ir_reads_a_callgrind_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'callgrind.out'
            path.write_text('positions: line\nevents: Ir\nob=/usr/lib/libpinyin.so.15\nfn=a\n10 100\n11 200\n')
            tools = snapshot.Tools()
            self.assertEqual(tools.object_ir(path, 'libpinyin'), 300)
            self.assertIsNone(tools.object_ir(path, 'libc'))


class OptionTests(Scratch):
    def test_defaults_and_every_option(self):
        self.assertEqual(snapshot.parse_options(['--out', 'x.json']), snapshot.Options('x.json', 'tkrzw'))
        parsed = snapshot.parse_options(['--out', 'x.json', '--backend', 'kyotocabinet'])
        self.assertEqual(parsed, snapshot.Options('x.json', 'kyotocabinet'))

    def test_the_last_value_of_a_repeated_option_wins(self):
        self.assertEqual(snapshot.parse_options(['--out', 'a', '--out', 'b']).out, 'b')


    def test_an_unknown_argument_stops_at_the_first_one(self):
        with captured() as (out, err), self.assertRaises(Finished) as finished:
            snapshot.parse_options(['--bogus', '--help'])
        self.assertEqual(finished.exception.status, 2)
        self.assertEqual(err.getvalue().decode(), 'fatal: unknown argument --bogus\n')
        self.assertEqual(out.getvalue(), b'')

    def test_help_prints_the_header_and_ends_the_run_with_zero_before_any_check(self):
        for argv in (['-h'], ['--help'], ['--out', 'x', '--help', '--bogus']):
            with self.subTest(argv=argv):
                tools = FakeTools()
                status, out, err = self.run_snapshot(argv, tools)
                self.assertEqual((status, err, tools.events), (0, '', []))
                self.assertEqual(out, snapshot.HELP)
        self.assertTrue(snapshot.HELP.startswith('# snapshot — measure the tree once'))
        # Lines 2-19 of the retired script: 17 comment lines and the blank line after them.
        self.assertTrue(snapshot.HELP.endswith('# Usage: just measure snapshot --out snapshot.json '
                                               '[--backend tkrzw]\n\n'))
        self.assertEqual(len(snapshot.HELP.splitlines()), 18)

    def test_out_is_required_and_a_trailing_option_has_no_value(self):
        for argv in ([], ['--out'], ['--out', ''], ['--backend', 'tkrzw']):
            with self.subTest(argv=argv):
                status, out, err = self.run_snapshot(argv)
                self.assertEqual((status, out, err), (2, '', 'fatal: --out is required\n'))

    def test_an_unsupported_backend_is_refused(self):
        for backend in ('bdb', '', 'TKRZW'):
            with self.subTest(backend=backend):
                status, out, err = self.run_snapshot(['--out', str(self.out), '--backend', backend])
                self.assertEqual((status, out, err), (2, '', f"fatal: unsupported backend '{backend}'\n"))

    def test_a_count_that_is_not_a_plain_decimal_is_refused_before_the_first_build(self):
        for value in ('2+3', '-1', 'abc', '08x'):
            with self.subTest(value=value):
                tools = FakeTools()
                status, out, err = self.run_snapshot(['--out', str(self.out)], tools, PERF_GATE_RSS_PROCESSES=value)
                self.assertEqual(status, 2)
                self.assertEqual(err, f"snapshot.sh: PERF_GATE_RSS_PROCESSES must be a decimal count (got '{value}')\n")
                self.assertEqual((out, tools.commands('run'), tools.commands('capture')), ('', [], []))

    def test_the_usage_checks_come_before_the_count_check(self):
        for argv, message in (([], 'fatal: --out is required\n'),
                              (['--out', 'x', '--backend', 'bdb'], "fatal: unsupported backend 'bdb'\n")):
            with self.subTest(argv=argv):
                status, out, err = self.run_snapshot(argv, PERF_GATE_RSS_PROCESSES='abc')
                self.assertEqual((status, err), (2, message))

    def test_a_missing_fixture_directory_is_refused_before_the_tools_are_looked_up(self):
        shutil.rmtree(self.root / 'fixtures/w3/kct')
        tools = FakeTools()
        status, out, err = self.run_snapshot(['--out', str(self.out), '--backend', 'kyotocabinet'], tools)
        self.assertEqual((status, out), (2, ''))
        self.assertEqual(err, f'fatal: fixture dir {self.root / "fixtures/w3/kct"} missing\n')
        self.assertEqual(tools.events, [])

    def test_each_missing_tool_is_named_in_the_preflight_order(self):
        for missing in snapshot.REQUIRED_TOOLS:
            with self.subTest(missing=missing):
                present = tuple(tool for tool in snapshot.REQUIRED_TOOLS if tool != missing)
                tools = FakeTools(present=present)
                status, out, err = self.run_snapshot(['--out', str(self.out)], tools)
                self.assertEqual((status, out, err), (2, '', f'fatal: {missing} not found\n'))
                self.assertEqual(tools.commands('run'), [])
        tools = FakeTools(present=())
        self.assertEqual(self.run_snapshot(['--out', str(self.out)], tools)[2], 'fatal: cargo not found\n')


class RunTests(Scratch):
    def happy(self, argv=None, tools=None, **environment):
        tools = tools or FakeTools()
        result = self.run_snapshot(argv or ['--out', str(self.out)], tools, **environment)
        return tools, result

    def test_the_run_builds_checks_strips_builds_the_fixture_measures_and_writes(self):
        tools, (status, out, err) = self.happy()
        self.assertEqual((status, err), (0, ''))
        self.assertEqual(out.splitlines(), [
            '==> building the shipped artifact (cinstall, the product path)',
            '==> building the alloc-count artifact (a fixture; nothing installs it)',
            '==> measuring',
            f'wrote {self.out}'])
        steps = [self.normal(tools, event[:2]) for event in tools.events
                 if event[0] in ('run', 'capture')]
        stage, work = '<work>/stage', '<work>'
        shipped, alloc = f'{stage}/usr/lib/libpinyin.so.15.0.0', f'{work}/target-alloc/release/libpinyin_capi.so'
        data = str(self.root / 'fixtures/w3/tkt')
        harness = f'{work}/bisect'
        pin = ['taskset', '-c', '3']
        callgrind = [*pin, 'valgrind', '--tool=callgrind', '--collect-atstart=no',
                     '--toggle-collect=bisect_perf_steady_unit', f'--callgrind-out-file={work}/callgrind.out',
                     harness, '--perf', shipped, data]
        self.assertEqual(steps, [
            ['run', [*snapshot.INSTALL, 'libpinyin', '--prefix=/usr', f'--destdir={stage}', '--',
                     '--no-default-features', '--features', 'tkrzw,shipped', '--target-dir', f'{work}/target-shipped']],
            ['run', ['strip', '--strip-all', shipped]],
            ['capture', ['nm', '-D', shipped]],
            ['run', ['cargo', 'build', '--locked', '--release', '-p', 'oxpinyin-capi', '--no-default-features',
                     '--features', 'tkrzw,alloc-count']],
            ['run', ['gcc', '-std=gnu11', '-O2', '-o', harness, 'tools/bisection/bisect.c', '-ldl']],
            ['capture', ['readelf', '-S', '-W', shipped]],
            ['capture', [*pin, harness, '--perf', alloc, data]],
            *[['capture', [*pin, harness, '--perf', shipped, data]]] * 5,
            ['capture', callgrind]])

    def test_the_builds_send_their_stdout_to_stderr_and_the_alloc_build_has_its_own_target_directory(self):
        tools, _ = self.happy()
        (install, strip, cargo, gcc) = [event for event in tools.events if event[0] == 'run']
        self.assertTrue(install[2]['stderr'] and cargo[2]['stderr'])
        self.assertFalse(strip[2]['stderr'] or gcc[2]['stderr'])
        self.assertEqual(cargo[2]['target'], str(tools.work / 'target-alloc'))
        self.assertIsNone(install[2]['target'])


    def test_a_failed_install_ends_the_run_with_its_status_before_anything_else(self):
        class Failing(FakeTools):
            def run(self, command, **options):
                if command[:len(snapshot.INSTALL)] == snapshot.INSTALL:
                    raise Finished(7)
                return super().run(command, **options)
        tools = Failing()
        status, out, err = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual((status, err), (7, ''))
        self.assertEqual(tools.commands('capture'), [])

    def test_an_install_that_leaves_no_library_is_refused(self):
        class Empty(FakeTools):
            def run(self, command, **options):
                if command[:len(snapshot.INSTALL)] == snapshot.INSTALL:
                    self.events.append(('run', [str(part) for part in command], {}))
                    return 0
                return super().run(command, **options)
        tools = Empty()
        status, out, err = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual((status, err), (2, 'fatal: cinstall produced no libpinyin.so.15.0.0\n'))

    def test_a_shipped_artifact_that_exports_the_alloc_readers_is_refused_before_the_fixture_build(self):
        tools = FakeTools(nm=b'0000 T oxpinyin_init\n0000 T oxpinyin_alloc_count\n')
        status, out, err = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual((status, err), (2, 'fatal: the shipped artifact exports oxpinyin_alloc_* symbols\n'))
        self.assertNotIn('cargo', [command[0] for command in tools.commands('run')])
        self.assertNotIn('building the alloc-count', out)

    def test_a_failed_nm_refuses_nothing(self):
        class FailedNm(FakeTools):
            def capture(self, command, **options):
                if list(command)[:2] == ['nm', '-D']:
                    self.events.append(('capture', [str(part) for part in command], {}))
                    return pool.Completed(1, b'oxpinyin_alloc_count\n', b'nm: bad file\n')
                return super().capture(command, **options)
        tools = FailedNm()
        status, out, err = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual((status, err), (0, 'nm: bad file\n'))

    def test_a_build_that_leaves_no_alloc_cdylib_is_refused(self):
        class Empty(FakeTools):
            def run(self, command, **options):
                if str(command[0]) == 'cargo':
                    self.events.append(('run', [str(part) for part in command], {}))
                    return 0
                return super().run(command, **options)
        status, out, err = self.run_snapshot(['--out', str(self.out)], Empty())
        self.assertEqual(status, 2)
        self.assertRegex(err, r'^fatal: no alloc-count cdylib at .*/target-alloc/release/libpinyin_capi\.so\n$')

    def test_the_document_has_the_sample_keys_and_values(self):
        tools, _ = self.happy()
        text = self.out.read_text()
        self.assertTrue(text.endswith('}\n'))
        doc = json.loads(text)
        data = self.root / 'fixtures/w3/tkt'
        # Path order: nested/two.bin sorts before one.bin; only the file names are hashed.
        digest = hashlib.sha256(b'two.bin' + b'two' + b'one.bin' + b'tkt').hexdigest()
        # The shipped library was stripped to 8 bytes before it was measured and described.
        self.assertEqual(doc, {
            'captured_utc': NOW, 'commit': COMMIT, 'event': 'schedule',
            'run_url': 'https://github.com/o/r/actions/runs/42',
            'environment': {
                'kernel': 'Linux 6.8.0 #1 SMP x86_64', 'runner_image': 'ubuntu24/20261001.1',
                'rustc': 'rustc 1.97.1', 'cc': 'cc (Debian 15.2.0) 15.2.0', 'glibc': 'ldd (Debian GLIBC 2.42) 2.42',
                'valgrind': 'valgrind-3.26.0', 'binutils': 'GNU readelf 2.45', 'backend': 'tkrzw',
                'fixture_sha256': digest, 'cpu_pinned': True, 'harness_commit': HARNESS_COMMIT},
            'artifacts': {
                'shipped_libpinyin': {
                    'recipe': 'just packaging install libpinyin --prefix=/usr --destdir=<stage> -- '
                              '--no-default-features --features tkrzw,shipped',
                    'sha256': hashlib.sha256(b'stripped').hexdigest(), 'bytes': 8},
                'alloccount_libpinyin': {
                    'recipe': 'cargo build --locked --release -p oxpinyin-capi --no-default-features '
                              '--features tkrzw,alloc-count'}},
            'metrics': {'section_sum': SECTION_SUM, 'stripped_size': 8,
                        'alloc_count_per_cycle': 25, 'alloc_bytes_per_cycle': 1024,
                        'rss_first_cycle_kib': 100, 'rss_last_cycle_kib': 200,
                        'ir_oxpinyin_object': 1234}})
        self.assertEqual(text, json.dumps(doc, indent=2, sort_keys=True) + '\n')
        self.assertTrue(data.is_dir())

    def test_the_kyotocabinet_backend_selects_its_fixture_and_features(self):
        tools, _ = self.happy(['--out', str(self.out), '--backend', 'kyotocabinet'])
        doc = json.loads(self.out.read_text())
        self.assertEqual(doc['environment']['backend'], 'kyotocabinet')
        self.assertEqual(doc['environment']['fixture_sha256'],
                         hashlib.sha256(b'two.bin' + b'two' + b'one.bin' + b'kct').hexdigest())
        self.assertIn('kyotocabinet,shipped', tools.commands('run')[0])
        self.assertIn('kyotocabinet,alloc-count', tools.commands('run')[2])
        self.assertEqual(doc['artifacts']['alloccount_libpinyin']['recipe'].rsplit(' ', 1)[1],
                         'kyotocabinet,alloc-count')

    def test_the_provenance_defaults_apply_when_the_ci_variables_are_absent(self):
        self.environment = {'PATH': '/usr/bin'}
        tools, _ = self.happy()
        doc = json.loads(self.out.read_text())
        self.assertEqual((doc['event'], doc['run_url'], doc['environment']['runner_image']),
                         ('local', None, 'local/local'))
        self.assertEqual(doc['environment']['cpu_pinned'], True)

    def test_an_empty_event_variable_stays_empty_as_the_heredoc_read_it(self):
        self.happy(GITHUB_EVENT_NAME='', GITHUB_RUN_ID='')
        doc = json.loads(self.out.read_text())
        self.assertEqual((doc['event'], doc['run_url']), ('', None))

    def test_the_measured_processes_are_pinned_with_the_perf_settings_and_no_loader_changes(self):
        tools, _ = self.happy(PERF_GATE_CYCLES='6', PERF_GATE_CG_CYCLES='2', PERF_GATE_RSS_PROCESSES='2',
                              PERF_CPU='5')
        captures = [event for event in tools.events if event[0] == 'capture' and '--perf' in event[1]]
        self.assertEqual(len(captures), 1 + 2 + 1)
        for event in captures:
            self.assertEqual(event[1][:3], ['taskset', '-c', '5'])
            self.assertNotIn('LD_LIBRARY_PATH', event[2])
            self.assertEqual((event[2]['PERF_BACKEND'], event[2]['PERF_MODE']), ('gate', 'speed'))
        self.assertEqual([event[2]['PERF_CYCLES'] for event in captures], ['6', '6', '6', '2'])

    def test_without_taskset_the_processes_run_unpinned_and_the_sample_says_so(self):
        tools = FakeTools(present=snapshot.REQUIRED_TOOLS + ('valgrind',))
        self.happy(tools=tools)
        doc = json.loads(self.out.read_text())
        self.assertIs(doc['environment']['cpu_pinned'], False)
        harness = [event[1] for event in tools.events if event[0] == 'capture' and '--perf' in event[1]]
        self.assertTrue(all(command[0].endswith('/bisect') or command[0] == 'valgrind' for command in harness))

    def test_without_valgrind_the_instruction_count_is_none_and_nothing_is_run(self):
        tools = FakeTools(present=snapshot.REQUIRED_TOOLS + ('taskset',))
        self.happy(tools=tools)
        doc = json.loads(self.out.read_text())
        self.assertIsNone(doc['metrics']['ir_oxpinyin_object'])
        self.assertEqual([event for event in tools.events if event[0] == 'object_ir'], [])
        self.assertFalse(any('valgrind' in command for command in tools.commands('capture')))

    def test_a_failed_callgrind_run_leaves_the_instruction_count_none(self):
        tools = FakeTools(valgrind_status=1)
        status, _, _ = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual(status, 0)
        self.assertIsNone(json.loads(self.out.read_text())['metrics']['ir_oxpinyin_object'])
        self.assertEqual([event for event in tools.events if event[0] == 'object_ir'], [])

    def test_an_object_without_instruction_records_is_none_not_zero(self):
        self.happy(tools=FakeTools(callgrind_ir=None))
        self.assertIsNone(json.loads(self.out.read_text())['metrics']['ir_oxpinyin_object'])

    def test_an_unreadable_callgrind_profile_ends_the_run_without_a_sample(self):
        class Unreadable(FakeTools):
            def object_ir(self, path, want):
                raise FileNotFoundError(2, 'No such file or directory')
        status, out, err = self.run_snapshot(['--out', str(self.out)], Unreadable())
        self.assertEqual(status, 1)
        self.assertEqual(err, 'callgrind-ir: [Errno 2] No such file or directory\n')
        self.assertFalse(self.out.exists())

    def test_a_missing_allocation_counter_records_none(self):
        self.happy(tools=FakeTools(alloc=row(cycles_ns=[1, 2, 3])))
        metrics = json.loads(self.out.read_text())['metrics']
        self.assertEqual((metrics['alloc_count_per_cycle'], metrics['alloc_bytes_per_cycle']), (None, None))

    def test_a_harness_that_prints_no_row_ends_the_run_with_status_1_and_no_sample(self):
        for alloc, reason in ((b'', 'Expecting value'), (b'not json\n', 'Expecting value')):
            with self.subTest(alloc=alloc):
                status, out, err = self.run_snapshot(['--out', str(self.out)], FakeTools(alloc=alloc))
                self.assertEqual(status, 1)
                self.assertTrue(err.startswith('FAIL: ' + reason), err)
                self.assertFalse(self.out.exists())

    def test_a_failed_alloc_process_with_a_row_ends_the_run_with_its_status(self):
        class Failing(FakeTools):
            def capture(self, command, **options):
                done = super().capture(command, **options)
                if '--perf' in [str(part) for part in command] and str(command[-2]).endswith('libpinyin_capi.so'):
                    return done._replace(status=9)
                return done
        status, _, _ = self.run_snapshot(['--out', str(self.out)], Failing())
        self.assertEqual(status, 9)
        self.assertFalse(self.out.exists())

    def test_the_rss_loop_runs_every_process_and_a_failed_one_in_the_middle_does_not_stop_it(self):
        rss = [(0, rss_row(100, 10)), (7, b''), (0, rss_row(300, 30)), (0, rss_row(500, 50)), (0, rss_row(700, 70))]
        tools = FakeTools(rss=rss)
        status, _, _ = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual(status, 0)
        metrics = json.loads(self.out.read_text())['metrics']
        # The median of 100, 300, 500, 700 and none from the failed process.
        self.assertEqual((metrics['rss_first_cycle_kib'], metrics['rss_last_cycle_kib']), (400, 40))
        self.assertEqual(tools.rss, [])

    def test_the_status_of_the_last_rss_process_is_the_status_of_the_run(self):
        rss = [(0, rss_row(100, 10))] * 4 + [(9, rss_row(100, 10))]
        status, _, _ = self.run_snapshot(['--out', str(self.out)], FakeTools(rss=rss))
        self.assertEqual(status, 9)
        self.assertFalse(self.out.exists())

    def test_zero_rss_processes_record_none(self):
        tools = FakeTools(rss=[])
        self.happy(tools=tools, PERF_GATE_RSS_PROCESSES='0')
        metrics = json.loads(self.out.read_text())['metrics']
        self.assertEqual((metrics['rss_first_cycle_kib'], metrics['rss_last_cycle_kib']), (None, None))

    def test_a_decimal_count_with_a_leading_zero_is_decimal(self):
        tools = FakeTools(rss=[(0, rss_row(1, 2))] * 8)
        self.happy(tools=tools, PERF_GATE_RSS_PROCESSES='08')
        self.assertEqual(tools.rss, [])

    def test_a_sample_without_provenance_is_refused_and_not_written(self):
        tools = FakeTools(answers={('git', 'rev-parse', 'HEAD'): 'unknown'})
        status, out, err = self.run_snapshot(['--out', str(self.out)], tools)
        self.assertEqual(status, 1)
        self.assertTrue(err.startswith('fatal: commit could not be resolved — '))
        self.assertNotIn('wrote', out)
        self.assertFalse(self.out.exists())

    def test_a_relative_out_is_written_below_the_checkout_and_the_directory_is_restored(self):
        before = os.getcwd()
        tools, (status, out, err) = self.happy(['--out', 'snapshot.json'])
        self.assertEqual(status, 0)
        self.assertEqual(out.splitlines()[-1], 'wrote snapshot.json')
        self.assertTrue((self.root / 'snapshot.json').is_file())
        self.assertEqual(os.getcwd(), before)

    def test_the_work_directory_is_removed_after_a_run_and_after_a_refusal(self):
        tools, _ = self.happy()
        self.assertFalse(tools.work.exists())
        refused = FakeTools(valgrind_status=1)
        self.run_snapshot(['--out', str(self.out)], refused)
        self.assertFalse(refused.work.exists())

    def test_the_recipe_is_registered_for_the_measure_command(self):
        self.assertIs(snapshot.RECIPES['snapshot'], snapshot.snapshot)
        self.assertIn('snapshot', recipes.NAMES)


@unittest.skipUnless(sys.platform.startswith('linux') and shutil.which('readelf') and shutil.which('gcc'),
                     'needs GNU readelf (binutils) and a GCC that builds ELF shared objects (Linux)')
class RealReadelfTests(unittest.TestCase):
    """The size parser against the real readelf, checked against the section headers read by struct."""

    def test_the_section_sum_equals_the_wanted_section_sizes_in_the_headers(self):
        with tempfile.TemporaryDirectory() as directory:
            source, library = Path(directory) / 'sample.c', Path(directory) / 'libsample.so'
            source.write_text('static const char *const names[] = {"one", "two", "three"};\n'
                              'const char *pick(int i) { return names[i % 3]; }\n'
                              'int twice(int i) { return pick(i)[0] + i; }\n')
            subprocess.run(['gcc', '-shared', '-fPIC', '-O2', '-o', str(library), str(source)], check=True,
                           capture_output=True)
            text = subprocess.run(['readelf', '-S', '-W', str(library)], check=True, capture_output=True,
                                  text=True).stdout
            layout = pages.object_layout(library)['elf_sections']
            expected = sum(end - start for start, end, label in layout if label in snapshot.WANTED_SECTIONS)
            self.assertGreater(expected, 0)
            self.assertEqual(snapshot.section_sum(text), expected)


if __name__ == '__main__':
    unittest.main()
