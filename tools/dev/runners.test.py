#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Portable runner policy tests with explicit command and host observations."""
from contextlib import ExitStack
from dataclasses import replace
from pathlib import Path
import inspect
import os
import json
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import host, test_support
from tools.dev.cell import CellError, Settings, library_path, patch_digest
from tools.dev.drivers import DriverServices
from tools.dev.pool import UnitContext
from tools.dev.runner_protocols import candidate_sequences, conf_body, text_order
from tools.dev.runner_recipes import (SURFACES, active_rows, candidate_blocks, explicit_compare, pinyin_needed,
                                      pinyin_resolved, residue, return_law, same_data,
                                      nbest_witness_cases, nbest_witness_results, ordinary)
from tools.dev.runner_state import verify_steps
from tools.dev.runners import Context
from tools.dev.system_data import SystemData, data_extension


class FixtureServices:
    """A command adapter, rather than replacements for subprocess or os functions."""
    def __init__(self, settings, output=b'fixture: identical\n', diagnostics=b''):
        self.settings = settings
        self.context = None
        self.launches = []
        self.output = output
        self.diagnostics = diagnostics

    def fork(self, settings, context, stdout, stderr):
        return self

    def command(self, argv, **options):
        command = list(map(str, argv))
        if command[0] == 'diff':
            same = Path(command[-2]).read_bytes() == Path(command[-1]).read_bytes()
            return subprocess.CompletedProcess(command, 0 if same else 1, b'', b'')
        if command[0] in ('pkg-config', 'gcc'):
            return subprocess.CompletedProcess(command, 0, b'', b'')
        raise AssertionError('unexpected fixture command: ' + command[0])

    def run(self, argv, obj, **options):
        self.launches.append((options['side'], options['environment'].get('LD_LIBRARY_PATH')))
        return subprocess.CompletedProcess(argv, 0, self.output, self.diagnostics)


class NbestWitnessTests(unittest.TestCase):
    """Synthetic records test validation, never infer the pin's predicates."""
    cases = {'fixture-on': dict(parser='full', scheme='1', options='0x1fe00198', input='a')}
    stdout = (b'case\tid=fixture-on\tparser=full\tscheme=1\toptions=0x1fe00198'
              b'\tinput=a\tparsed=1\tguess=1\ncases\tcompleted=1\n')
    empty = (b'precondition\tcase=fixture-on\tsite=S1\tstep=0\tcount=1\tpairs=0\twitness=0\n'
             b'precondition\tcase=fixture-on\tsite=S2\tstep=1\tcount=1\tpairs=0\twitness=0\n'
             b'precondition\tcase=fixture-on\tsite=S3\tstep=1\tcount=1\twitness=0\n')

    def test_fixed_case_set(self):
        cases = nbest_witness_cases(ROOT / 'tools/bisection/nbest-precondition-cases.tsv')
        self.assertEqual(len(cases), 277)
        self.assertEqual(cases['surface-short-dia'],
                         dict(parser='full', scheme='1', options='0x1fe00018', input='dia'))
        for category, count in [('divided', 40), ('resplit', 170), ('fixture', 58), ('double', 8)]:
            self.assertEqual(sum(name.startswith(category + '-') for name in cases), count)
        for name, row in cases.items():
            if name.endswith('-on'):
                other = cases[name.removesuffix('-on') + '-off']
                self.assertEqual(row | {'options': '0x1fe00018'}, other)

    def test_case_file_rejects_duplicates_and_invalid_parameters(self):
        good = 'example\tfull\t1\t0x1fe00198\txian\n'
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'cases.tsv'
            for content in ['', good * 2, good.replace('full', 'unknown'),
                            good.replace('\t1\t', '\t2\t'), good.replace('1fe00198', '0'),
                            good.replace('xian', '西安'), good.replace('\txian', '\txi\tan')]:
                with self.subTest(content=content):
                    path.write_text(content)
                    with self.assertRaises(CellError):
                        nbest_witness_cases(path)

    def test_no_witness_and_exact_hex_witnesses(self):
        summary = nbest_witness_results(self.cases, self.stdout,
                                       b'open nbest-witness-user-test/user.conf failed.' + self.empty)
        self.assertTrue(all(row['calls'] == 1 and row['first'] is None for row in summary.values()))
        values = ''.join(f'\t{label}_position={index}\t{label}_length=2'
                         f'\t{label}_poss=-0x1.23456p+4\t{label}_prev=0x00000001'
                         f'\t{label}_token=0x01000002\t{label}_slot={index % 2}'
                         for index, label in enumerate('abc'))
        pair = values[:values.index('\tc_position')]
        lines = [f'precondition\tcase=fixture-on\tsite={site}\tstep={step}'
                 f'\tcount=3\tpairs=2\twitness=1{pair}\tless_ab=1\tless_ba=1'
                 for site, step in [('S1', 0), ('S2', 1)]]
        lines.append('precondition\tcase=fixture-on\tsite=S3\tstep=1\tcount=3\twitness=1' + values +
                     '\tcompare_ab=0\tcompare_ba=0\tcompare_bc=0\tcompare_cb=0\tcompare_ac=1\tcompare_ca=-1')
        summary = nbest_witness_results(self.cases, self.stdout, '\n'.join(lines).encode())
        for site, line in zip(('S1', 'S2', 'S3'), lines):
            self.assertEqual(summary[site]['first'], line)
            self.assertEqual(summary[site]['witnessed_calls'], 1)
        self.assertEqual(summary['S1']['pairs'], 2)
        # Deliberately synthetic equal floats above prove the runner trusts the
        # diagnostic predicate results instead of decimal/host recomputation.
        for invalid in [lines[0].replace('less_ab=1', 'less_ab=0'),
                        lines[0].replace('a_poss=-0x1.23456p+4', 'a_poss=-18.2'),
                        lines[0].replace('pairs=2', 'pairs=4'),
                        lines[0].replace('b_position=1', 'b_position=0')]:
            with self.subTest(invalid=invalid), self.assertRaises(CellError):
                nbest_witness_results(self.cases, self.stdout, '\n'.join([invalid, *lines[1:]]).encode())

    def test_incomplete_or_duplicate_capture_fails(self):
        for stdout, stderr in [(self.stdout.replace(b'guess=1', b'guess=0'), self.empty),
                               (self.stdout.replace(b'completed=1', b'completed=2'), self.empty),
                               (self.stdout, self.empty.split(b'\n', 1)[1]),
                               (self.stdout, self.empty + self.empty),
                               (self.stdout, self.empty.replace(b'step=0', b'step=2'))]:
            with self.subTest(stdout=stdout, stderr=stderr), self.assertRaises(CellError):
                nbest_witness_results(self.cases, stdout, stderr)

    def test_diagnostic_dispatch_runs_only_pin_and_checks_identity(self):
        with tempfile.TemporaryDirectory() as scratch, ExitStack() as stack:
            root = Path(scratch).resolve()
            cases = root / 'cases.tsv'
            cases.write_text('fixture-on\tfull\t1\t0x1fe00198\ta\n')
            prefix, data = root / 'pin', root / 'data'
            prefix.mkdir()
            data.mkdir()
            (data / 'table.conf').write_text('database format:BerkeleyDB\n')
            (data / 'bigram.db').touch()
            settings = Settings.read({'PINYIN_ORACLE_DBM': 'bdb'})
            digest = patch_digest(ROOT / 'tools/bisection/patches/nbest-tails-dump')
            identity = f'{settings.expected_pin}+patches-{digest}\npatches_manifest_sha256={digest}\ndbm=BerkeleyDB\n'
            (prefix / 'oracle-pin.txt').write_text(identity)
            diagnostic = b'open nbest-witness-user-test/user.conf failed.' + self.empty
            services = FixtureServices(settings, self.stdout, diagnostic)
            ctx = Context(settings, services, stack, root, 'nbest-train-diff', events=[])
            args = ['--pin-witness-cases', str(cases), '--prefix', str(prefix), '--data', str(data)]
            with mock.patch('tools.dev.runner_recipes.build_subject', side_effect=AssertionError('subject build')), \
                    mock.patch.object(Context, 'compile', return_value=root / 'driver'), \
                    mock.patch.object(services, 'run', wraps=services.run) as run:
                self.assertEqual(ordinary(ctx, args), 0)
                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args.kwargs['side'], 'pin')
                self.assertEqual(run.call_args.args[0][-2:], ['--cases', str(cases)])
                self.assertIn((True, diagnostic), ctx.events)
                for bad in [identity.replace(digest, 'bad'), identity.replace('BerkeleyDB', 'KyotoCabinet')]:
                    (prefix / 'oracle-pin.txt').write_text(bad)
                    with self.assertRaises(CellError):
                        ordinary(ctx, args)
                self.assertEqual(run.call_count, 1)
                ctx.settings = replace(settings, cell='kc')
                with self.assertRaises(CellError):
                    ordinary(ctx, args)


class NbestSurfaceTests(unittest.TestCase):
    def test_empty_case_is_rejected_before_build_or_pin_lookup(self):
        with ExitStack() as stack:
            settings = Settings.read({})
            ctx = Context(settings, FixtureServices(settings), stack, ROOT, 'nbest-train-diff')
            with mock.patch('tools.dev.runner_recipes.build_subject', side_effect=AssertionError('unexpected build')), \
                 mock.patch.object(Context, 'pin', side_effect=AssertionError('unexpected pin lookup')):
                with self.assertRaisesRegex(CellError, 'unknown n-best surface case:'):
                    ordinary(ctx, ['--surface-case', ''])

    def test_selected_case_reaches_both_libraries_without_requiring_exports(self):
        with tempfile.TemporaryDirectory() as scratch, ExitStack() as stack:
            root = Path(scratch).resolve()
            pin, data, subject = root / 'pin', root / 'data', root / 'subject.so'
            (pin / 'lib/libpinyin/data').mkdir(parents=True)
            (pin / 'lib/libpinyin.so').touch()
            (pin / 'lib/libpinyin/data/bigram.db').touch()
            data.mkdir()
            for name in ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'interpolation2.text'):
                (data / name).touch()
            (data / 'table.conf').write_text('database format:BerkeleyDB\n')
            subject.touch()
            settings = Settings.read(dict(PINYIN_ORACLE_PREFIX=str(pin), NBEST_CAPI_SYSTEM=str(data),
                                          OXPINYIN_CAPI_SO=str(subject)))
            (pin / 'oracle-pin.txt').write_text(settings.expected_pin + '\n')
            output = b'probe:surface-short-dia guess=1\nprobe:surface-short-dia cand[0]=NBEST/0/x\n'

            class SurfaceServices(FixtureServices):
                def run(self, argv, obj, **options):
                    self.arguments.append(list(map(str, argv[3:])))
                    return super().run(argv, obj, **options)

            services = SurfaceServices(settings, output=output)
            services.arguments = []
            ctx = Context(settings, services, stack, root, 'nbest-train-diff', events=[])
            self.assertEqual(ordinary(ctx, ['--surface-case', 'surface-short-dia']), 0)
            expected = ['--surface-case', 'surface-short-dia', 'full', '1', '0x1fe00018', 'dia']
            self.assertEqual(services.arguments, [expected, expected])
            rendered = b''.join(data for _, data in ctx.events)
            self.assertIn(b'case:surface-short-dia IDENTICAL', rendered)
            self.assertEqual([side for side, _ in services.launches], ['subject', 'pin'])
            with self.assertRaisesRegex(CellError, 'unknown n-best surface case'):
                ordinary(ctx, ['--surface-case', 'unknown-case'])
            self.assertEqual(len(services.launches), 2)


class LibraryPathTests(unittest.TestCase):
    def test_all_legacy_library_path_cases_reach_both_driver_launches(self):
        cases = [('relative', './lib', '{cwd}/./lib'), ('leading', ':/absolute/lib', '{cwd}:/absolute/lib'),
                 ('trailing', '/absolute/lib:', '/absolute/lib:{cwd}'),
                 ('doubled', '/absolute/lib::lib', '/absolute/lib:{cwd}:{cwd}/lib'),
                 ('empty_entries', ':', '{cwd}:{cwd}'), ('absolute', '/first/lib:/second/lib', '/first/lib:/second/lib'),
                 ('order', 'last:/absolute/lib:./first', '{cwd}/last:/absolute/lib:{cwd}/./first'),
                 ('spaces', 'relative lib:/absolute path/lib', '{cwd}/relative lib:/absolute path/lib'),
                 *[(token, 'prefix/' + token + '/lib', 'prefix/' + token + '/lib')
                   for token in ('$ORIGIN', '${ORIGIN}', '$LIB', '${LIB}', '$PLATFORM', '${PLATFORM}')],
                 ('empty', '', ''), ('unset', None, '')]
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            caller = root / 'caller root'
            caller.mkdir()
            data = root / 'data'
            data.mkdir()
            (data / 'gb_char.bin').touch()
            pin, subject = root / 'pin/libpinyin.so', root / 'subject/libpinyin.so'
            for file in (pin, subject):
                file.parent.mkdir()
                file.touch()
            for name, value, expected in cases:
                with self.subTest(case=name), ExitStack() as stack:
                    env = {} if value is None else {'LD_LIBRARY_PATH': value}
                    settings = Settings.read(env)
                    services = FixtureServices(settings)
                    ctx = Context(settings, services, stack, caller, 'same-data-dir-diff')
                    self.assertEqual(same_data(ctx, [str(pin), str(subject), str(data), 'key-surface-diff']), 0)
                    inherited = expected.replace('{cwd}', str(caller))
                    suffix = ':' + inherited if inherited else ''
                    self.assertEqual(services.launches, [('oracle', str(pin.parent) + suffix),
                                                        ('oxpinyin', str(subject.parent) + suffix)])


class PolicyTests(unittest.TestCase):
    def test_export_gate_retains_the_empty_summary_failure(self):
        with tempfile.TemporaryDirectory() as scratch, ExitStack() as stack:
            root = Path(scratch)
            pin, subject = root / 'pin.so', root / 'subject.so'
            pin.touch()
            subject.touch()
            settings = Settings.read({})
            ctx = Context(settings, FixtureServices(settings, output=b''), stack, root, 'export-diff')
            self.assertEqual(explicit_compare(ctx, [str(pin), str(subject), str(root)]), 1)

    def test_elf_observations_are_parsed_without_a_linux_host(self):
        self.assertEqual(pinyin_needed(' (NEEDED) Shared library: [libpinyin.so.15]\n Shared library: [libc.so.6]'), 'libpinyin.so.15')
        self.assertEqual(pinyin_resolved('libpinyin.so.15 => /build/lib/libpinyin.so.15 (0x1)'), '/build/lib/libpinyin.so.15')
        self.assertEqual(pinyin_needed('Shared library: [libpinyin_capi.so]'), 'libpinyin_capi.so')

    def test_bigram_return_law_requires_final_false_and_nonvacuity(self):
        self.assertTrue(return_law(b'', 'empty'))
        self.assertFalse(return_law(b'', 'one'))
        self.assertTrue(return_law(b'train x: true\nrow x\tget=true\nrow y\tget=false\n', 'two'))
        self.assertFalse(return_law(b'row x\tget=true\n', 'one'))
        self.assertFalse(return_law(b'row (null)\tget=false\n', 'one'))

    def test_candidate_blocks_preserve_order_and_trailing_newlines(self):
        self.assertEqual(candidate_blocks(b'== first\nx\n== second\ny'), {'first': [b'x\n'], 'second': [b'y']})

    def test_option_comparison_keeps_set_length_and_w12_controls(self):
        pin = 'input=ni aux=x\ncand[0]=你好\ncand[1]=世界\n'.encode()
        tie = 'input=ni aux=x\ncand[0]=世界\ncand[1]=你好\n'.encode()
        shorter = 'input=ni\ncand[0]=你\ncand[1]=好世界\n'.encode()
        self.assertEqual(text_order(pin, pin)[0], 'identical')
        self.assertEqual(text_order(pin, tie)[0], 'tie-order')
        self.assertEqual(text_order(pin, shorter)[0], 'stop')
        self.assertEqual(text_order(pin.replace(b'input=ni', b'input=cang'), b'input=cang\n')[0], 'w12-residual')
        self.assertEqual(candidate_sequences(pin), {'ni': '你好|世界'})

    def test_conf_mutations_keep_raw_bytes_and_decimal_tokens(self):
        identity = [b'binary format version:7', b'model data version:14', b'database format:BerkeleyDB']
        self.assertIn(b'4294967303', conf_body('binver-wrap', identity))
        self.assertEqual(conf_body('empty', identity), b'')
        self.assertFalse(conf_body('no-newline', identity).endswith(b'\n'))
        self.assertIn(b'database format:' + b'A' * 255 + b'open counter:5\n', conf_body('trunc-counter', identity))

    def test_surface_prerequisites_refuse_vacuous_output(self):
        self.assertFalse(active_rows(b'', SURFACES['phrase-surface-diff'].patterns))
        self.assertTrue(active_rows('segment|你好中国|1|n=4|\nsegment|你好，世界。|0|n=6|\nprefix|nihaoshijie|你好|1|\nreset|rok=1|n=0|\n'.encode(), SURFACES['phrase-surface-diff'].patterns))
        self.assertTrue(verify_steps('pinyin', 'one-learns', b'scenario: one-learns\n'))


class DataTests(unittest.TestCase):
    def test_native_and_peer_layouts_require_the_selected_backend(self):
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            for file in ('gb_char.bin', 'table.conf', 'pinyin_index.bin', 'phrase_index.bin', 'bigram.db'):
                (directory / file).touch()
            (directory / 'table.conf').write_text('database format:BerkeleyDB\n')
            settings = Settings.read({'OXPINYIN_SYSTEM_DIR': str(directory)})
            service = SystemData(settings)
            self.assertEqual(data_extension(directory), 'db')
            self.assertEqual(service.detect(directory), 'bin')
            self.assertEqual(service.resolve('SPECIFIC', 'fixture'), directory)
            for stem in ('pinyin_index', 'phrase_index'):
                (directory / (stem + '.db')).touch()
            self.assertEqual(service.detect(directory), 'db')
            (directory / 'table.conf').write_text('database format:Tkrzw\n')
            with self.assertRaisesRegex(CellError, 'BACKEND MISMATCH'):
                service.detect(directory)

    def test_specific_override_wins_and_incomplete_data_fails_closed(self):
        with tempfile.TemporaryDirectory() as scratch:
            settings = Settings.read({'SPECIFIC': scratch, 'OXPINYIN_SYSTEM_DIR': '/wrong'})
            with self.assertRaisesRegex(CellError, 'system directory is incomplete') as error:
                SystemData(settings).resolve('SPECIFIC', 'fixture')
            self.assertEqual(error.exception.status, 3)


class LauncherTests(unittest.TestCase):
    def test_pool_launch_keeps_cwd_and_merges_streams(self):
        context = UnitContext('fixture', threading.Event(), environment=dict(os.environ))
        with tempfile.TemporaryDirectory() as scratch:
            result = context.launch([sys.executable, '-c', 'import os,sys; print(os.getcwd(), flush=True); print("diagnostic", file=sys.stderr)'],
                                    cwd=Path(scratch), merge_stderr=True)
            self.assertEqual(result.status, 0)
            self.assertEqual(result.stderr, b'')
            self.assertEqual(result.stdout.decode().splitlines(), [str(Path(scratch).resolve()), 'diagnostic'])


@unittest.skipUnless(sys.platform == 'linux' and Path('/proc/self/stat').is_file(),
                     'Linux /proc process-liveness and flock handoff facilities')
class LinuxOwnershipTests(unittest.TestCase):
    def live(self, pid):
        try:
            return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z'
        except FileNotFoundError:
            return False

    def exercise(self, terminate):
        inner = '''
from contextlib import ExitStack
from pathlib import Path
import json, os, sys
from tools.dev import pool, slot
from tools.dev.cell import Settings
from tools.dev.drivers import DriverServices
from tools.dev.runners import Context
with slot.cancellation_handlers(), ExitStack() as stack:
    settings = Settings.read(os.environ)
    context = pool.UnitContext('fixture', slot.CANCELLED, environment=dict(settings.environment), fds=slot.inherited_fds())
    assert context.fds, 'the runner lost the inherited slot descriptor'
    services = DriverServices(settings, context)
    work = Context(settings, services, stack, Path.cwd(), 'fixture').scratch()
    Path(sys.argv[1]).write_text(json.dumps(dict(pid=os.getpid(), work=str(work))))
    code = 'import json,os,subprocess,sys,time; child=subprocess.Popen([sys.executable,"-c","import time; time.sleep(60)"]); open(sys.argv[1],"w").write(json.dumps([os.getpid(),child.pid])); time.sleep(60)'
    try:
        services.command([sys.executable, '-c', code, sys.argv[2]])
    except pool.UnitStopped:
        result = slot.cancellation_status()
    else:
        result = 0
raise SystemExit(result)
'''
        outer = ("import sys; from tools.dev import slot; "
                 f"code={inner!r}; "
                 "result=slot.run_command([sys.executable,'-X','lazy_imports=normal','-c',code,*sys.argv[1:]]); "
                 "raise SystemExit(128-result if result<0 else result)")
        with tempfile.TemporaryDirectory() as scratch:
            directory = Path(scratch)
            record, children = directory / 'record', directory / 'children'
            env = dict(os.environ, OXPINYIN_LOCK_DIR=str(directory / 'locks'), OXPINYIN_HEAVY_SLOTS='1',
                       OXPINYIN_RESOURCE_WAIT_SECONDS='2')
            env.pop('OXPINYIN_HEAVY_SLOT_HELD', None)
            process = subprocess.Popen([sys.executable, '-X', 'lazy_imports=normal', '-c', outer, str(record), str(children)],
                                       cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            pids = []
            try:
                deadline = time.monotonic() + 10
                while not children.exists() and time.monotonic() < deadline and process.poll() is None:
                    time.sleep(0.02)
                self.assertTrue(children.exists(), 'the admitted driver did not start')
                pids = json.loads(children.read_text())
                owner = json.loads(record.read_text())
                os.kill(owner['pid'], terminate)
                process.communicate(timeout=10)
                self.assertEqual(process.returncode, 128 + terminate)
                if terminate == signal.SIGTERM:
                    self.assertFalse(Path(owner['work']).exists(), 'cancelled runner kept its private scratch')
                    self.assertFalse(any(self.live(pid) for pid in pids), 'cancelled runner kept a live descendant')
                else:
                    self.assertTrue(self.live(pids[0]))
                    contender = ("import io; from tools.dev import slot; holder=slot.HeavySlot('contender',out=io.StringIO()); "
                                 "acquired=holder.try_acquire(); print('acquired' if acquired else 'blocked'); holder.__exit__()")
                    result = subprocess.run([sys.executable, '-c', contender], cwd=ROOT, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, 'blocked\n', 'a killed wrapper released its live driver’s slot')
                    os.killpg(pids[0], signal.SIGKILL)
                    deadline = time.monotonic() + 5
                    while self.live(pids[0]) and time.monotonic() < deadline:
                        time.sleep(0.02)
                    result = subprocess.run([sys.executable, '-c', contender], cwd=ROOT, env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(result.stdout, 'acquired\n', 'the ended driver leaked its slot descriptor')
            finally:
                for pid in pids[:1]:
                    try:
                        os.killpg(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                if process.poll() is None:
                    process.kill()
                process.communicate(timeout=10)
                if record.exists():
                    import shutil
                    shutil.rmtree(json.loads(record.read_text())['work'], ignore_errors=True)

    def test_sigterm_reaps_descendants_and_removes_scratch(self):
        self.exercise(signal.SIGTERM)

    def test_sigkill_keeps_the_slot_until_the_driver_ends(self):
        self.exercise(signal.SIGKILL)


def observe_group_live(group, timeout=1):
    """Retry only unknown snapshots; preserve any executing survivor immediately."""
    deadline = time.monotonic() + timeout
    while True:
        live = host.process_group_live(group)
        if live is not None:
            return live
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return None
        time.sleep(min(0.01, remaining))


# A residue-a-tail-diff runner whose driver steps are replaced, so that the next
# real child is the RESIDUE_A_PROBE build. It records what became of that build.
RESIDUE_RUNNER = '''
import json, os, signal, subprocess, sys, time
from pathlib import Path
from unittest import mock
from tools.dev import host, runners
from tools.dev.cell import Settings
''' + inspect.getsource(observe_group_live) + '''
fixture = json.loads(sys.argv[1])
def run(driver, so, args, *, side, **options):
    if side == 'pin' and fixture['signal_before_build']:
        os.kill(os.getpid(), signal.SIGTERM)
    return subprocess.CompletedProcess([str(driver)], 0, b'row\\n', b'')
with mock.patch.object(host, 'resource_snapshot', return_value=fixture['resources']), \\
        mock.patch.object(runners.Context, 'compile', return_value=Path(fixture['driver'])), \\
        mock.patch.object(runners.Context, 'run', side_effect=run), \\
        mock.patch.object(Settings, 'artifact', return_value=Path(fixture['capi'])):
    seen = dict(status=runners.main(['residue-a-tail-diff']))
pids = Path(os.environ['FAKE_CARGO_STATE'], 'pids')
if pids.exists():
    leader = int(pids.read_text().split()[0])
    try:
        os.waitpid(leader, os.WNOHANG)
        seen['reaped'] = False
    except ChildProcessError:
        seen['reaped'] = True
    seen['group_live'] = observe_group_live(leader)
Path(fixture['result']).write_text(json.dumps(seen))
'''

# The probe build: it leaves a second member in its process group, and when asked
# it signals the runner that launched it while it is still running.
FAKE_PROBE_CARGO = '''#!/bin/sh
sleep 30 &
printf '%s %s\\n' "$$" "$!" > "$FAKE_CARGO_STATE/pids"
if [ "${FAKE_CARGO_SIGNAL_PARENT:-}" = 1 ]; then kill -TERM "$PPID"; fi
i=0
while [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
'''


class ResidueProbeFixture(unittest.TestCase):
    """A residue-a-tail-diff runner whose next real child is the RESIDUE_A_PROBE build."""

    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        root = Path(scratch.name)
        (root / 'bin').mkdir()
        cargo = root / 'bin/cargo'
        cargo.write_text(FAKE_PROBE_CARGO)
        cargo.chmod(0o755)
        self.state = root / 'state'
        self.state.mkdir()
        self.addCleanup(self.kill_leftovers)
        prefix = root / 'oracle'
        (prefix / 'lib/libpinyin/data').mkdir(parents=True)
        (prefix / 'lib/libpinyin.so').write_bytes(b'')
        (prefix / 'lib/libpinyin/data/bigram.db').write_bytes(b'')
        (prefix / 'oracle-patches.sha256').write_text('pin-bigram-export-strjoinv.patch\n')
        self.env = {name: value for name, value in os.environ.items()
                    if name not in ('OXPINYIN_HEAVY_SLOT_HELD', 'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO',
                                    'RESIDUE_A_CAPI_SO', 'OXPINYIN_NBEST_TAIL_PROBE', 'RESIDUE_A_DUMP_PREFIX',
                                    'CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', 'UV_PYTHON', 'PYTHONHOME', 'PYTHONPATH')}
        self.env.update(PATH=str(root / 'bin') + os.pathsep + os.environ['PATH'],
                        OXPINYIN_LOCK_DIR=str(root / 'locks'), OXPINYIN_HEAVY_SLOTS='1',
                        OXPINYIN_RESOURCE_WAIT_SECONDS='5', CARGO_TARGET_DIR=str(root / 'target'),
                        PINYIN_ORACLE_DBM='bdb', PINYIN_ORACLE_PREFIX=str(prefix), RESIDUE_A_PROBE='1',
                        RESIDUE_A_SAME_DIR='1', RESIDUE_A_OUT=str(root / 'out'), FAKE_CARGO_STATE=str(self.state))
        digest = patch_digest(ROOT / 'tools/bisection/patches/bigram-export-strjoinv')
        (prefix / 'oracle-pin.txt').write_text(f'{Settings.read(self.env).expected_pin}+patches-{digest}\n'
                                               f'patches_manifest_sha256={digest}\n')
        self.fixture = dict(driver=str(root / 'driver'), capi=str(root / 'libpinyin_capi.so'),
                            result=str(root / 'result.json'),
                            resources=dict(available_mib=12000, total_mib=16384, cpus=4, cargo_cpus=4,
                                           memory_limit_mib=None, notes=['synthetic resources']))

    def kill_leftovers(self):
        pids = self.state / 'pids'
        if pids.exists():
            try:
                os.killpg(int(pids.read_text().split()[0]), signal.SIGKILL)
            except ProcessLookupError:
                pass


class GroupExitObservationTests(unittest.TestCase):
    def test_retries_unknown_but_returns_known_observations_immediately(self):
        for observations in ([None, False], [True, False]):
            with self.subTest(observations=observations), \
                    mock.patch.object(host, 'process_group_live', side_effect=observations) as live, \
                    mock.patch.object(time, 'monotonic', return_value=0), \
                    mock.patch.object(time, 'sleep') as sleep:
                observed = observe_group_live(123)
                if observations[0] is True:
                    self.assertIs(observed, True)
                    live.assert_called_once_with(123)
                    sleep.assert_not_called()
                    with self.assertRaises(AssertionError):
                        self.assertIs(observed, False)
                else:
                    self.assertIs(observed, False)
                    self.assertEqual(live.call_args_list, [mock.call(123)] * 2)
                    sleep.assert_called_once_with(0.01)

    def test_unknown_until_deadline_remains_a_failure(self):
        with mock.patch.object(host, 'process_group_live', return_value=None) as live, \
                mock.patch.object(time, 'monotonic', side_effect=[0, 0, 1]), \
                mock.patch.object(time, 'sleep') as sleep:
            observed = observe_group_live(123)
        self.assertIsNone(observed)
        self.assertEqual(live.call_args_list, [mock.call(123)] * 2)
        sleep.assert_called_once_with(0.01)
        with self.assertRaises(AssertionError):
            self.assertIs(observed, False)


@unittest.skipIf(sys.platform == 'win32', 'Unix process groups and signals')
class ResidueProbeCancellationTests(ResidueProbeFixture):
    """SIGTERM before or during the residue probe build cancels the runner with status 128 + SIGTERM."""

    def run_residue(self, *, signal_before_build=False, signal_from_build=False):
        env = dict(self.env, FAKE_CARGO_SIGNAL_PARENT='1' if signal_from_build else '0')
        fixture = dict(self.fixture, signal_before_build=signal_before_build)
        result = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', RESIDUE_RUNNER,
                                 json.dumps(fixture)], cwd=ROOT, env=env, capture_output=True, text=True,
                                timeout=60)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('--- runtime-side probe ---', result.stdout)
        return json.loads(Path(fixture['result']).read_text()), result

    def test_sigterm_during_the_probe_build_stops_and_reaps_its_process_group(self):
        seen, result = self.run_residue(signal_from_build=True)
        self.assertEqual(seen['status'], 128 + signal.SIGTERM, result.stderr)
        self.assertTrue(seen['reaped'], 'the probe build was not reaped by the runner')
        self.assertIs(seen['group_live'], False, 'a member of the probe build group outlived the runner '
                      'or its termination could not be observed')

    def test_sigterm_before_the_probe_build_cancels_without_starting_it(self):
        seen, result = self.run_residue(signal_before_build=True)
        self.assertEqual(seen['status'], 128 + signal.SIGTERM, result.stderr)
        self.assertFalse((self.state / 'pids').exists(), 'the probe build started after SIGTERM')


class ProbeServices(FixtureServices):
    """FixtureServices that also runs the probe the recipe built."""
    def command(self, argv, **options):
        if Path(str(argv[0])).name == 'nbest_tail_probe':
            return subprocess.CompletedProcess(list(map(str, argv)), 0, b'probe\n', b'')
        return super().command(argv, **options)


class ResidueProbeWindowsTests(ResidueProbeFixture):
    def test_windows_probe_build_takes_the_job_object_path_without_pass_fds(self):
        settings = Settings.read(self.env)
        with ExitStack() as stack, test_support.windows_launch() as (launch, jobs, ntdll), \
                mock.patch.object(Settings, 'artifact', return_value=Path(self.fixture['capi'])):
            ctx = Context(settings, ProbeServices(settings), stack, ROOT, 'residue-a-tail-diff', events=[])
            self.assertEqual(residue(ctx, []), 0, b''.join(data for _, data in ctx.events).decode())
        test_support.assert_windows_launches(self, launch, jobs, ntdll, cwd=ROOT)
        [build] = launch.call_args_list
        self.assertEqual(build.args[0][:2], ['cargo', 'build'])
        self.assertIn('nbest_tail_probe', build.args[0])


if __name__ == '__main__':
    unittest.main()
