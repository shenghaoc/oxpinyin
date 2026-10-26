#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Portable runner policy tests with explicit command and host observations."""
from contextlib import ExitStack
from dataclasses import replace
from pathlib import Path
import os
import json
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev.cell import CellError, Settings, library_path
from tools.dev.drivers import DriverServices
from tools.dev.pool import UnitContext
from tools.dev.runner_protocols import candidate_sequences, conf_body, text_order
from tools.dev.runner_recipes import (SURFACES, active_rows, candidate_blocks, explicit_compare, pinyin_needed,
                                      pinyin_resolved, return_law, same_data)
from tools.dev.runner_state import verify_steps
from tools.dev.runners import Context
from tools.dev.system_data import SystemData, data_extension


class FixtureServices:
    """A command adapter, rather than replacements for subprocess or os functions."""
    def __init__(self, settings, output=b'fixture: identical\n'):
        self.settings = settings
        self.context = None
        self.launches = []
        self.output = output

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
        return subprocess.CompletedProcess(argv, 0, self.output, b'')


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


if __name__ == '__main__':
    unittest.main()
