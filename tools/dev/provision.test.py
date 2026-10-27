#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Synthetic provisioning fixtures; U6_BASE also compares preserved shell entry points."""
import contextlib
import hashlib
import io
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import provision, slot
from tools.dev.compat.identity import digest_file
from tools.dev.runners import Finished


class ProvisionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.work = Path(temporary.name)
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        # These tests never inspect a real environment/profile/CA or inherited secrets.
        self.env = dict(PATH=str(self.bin) + ':' + os.defpath, HOME=str(self.work), LC_ALL='C',
                        TMPDIR=str(self.work), OXPINYIN_ORACLE_ENV=str(self.work / 'oracle.env'))
        self.service = provision.Provision(root=ROOT, env=self.env)
        self.service.cache = self.work / "cache '$`"
        self.service.cache.mkdir()
        self.service.model_cache = self.work / 'model20'
        self.service.uv_dir = str(self.work / "uv '$`")
        self.service.pin = self.work / 'pin'
        self.service.pin.touch()
        self.service.platform = 'linux/amd64'
        self.service.cells = ['bdb', 'kc', 'tkrzw']
        self.service.tkrzw_local = True
        self.service.log = lambda value: print(value, file=sys.stderr)
        self.base = Path(os.environ['U6_BASE']) if os.environ.get('U6_BASE') else None
        for cell in provision.CELLS:
            self.fixture(cell)

    def fixture(self, cell):
        prefix = self.service.oracle_root / cell
        data = prefix / 'lib/libpinyin/data'
        data.mkdir(parents=True)
        for library in ('libpinyin', 'libzhuyin'):
            path = prefix / 'lib' / (library + '.so')
            path.write_text(f'{cell} {library}\n')
            with self.service.pin.open('a') as pin:
                pin.write(f'{library}_sha256_{cell}={digest_file(path)}\n')
        (data / 'table.conf').write_text(cell + '\n')
        (prefix / 'oracle-data.sha256').write_text(f'{digest_file(data / "table.conf")}  lib/libpinyin/data/table.conf\n')
        unstable = []
        for name in ('bigram.db', 'phrase_index.bin', 'pinyin_index.bin', 'punct.bin', 'addon_phrase_index.bin', 'addon_pinyin_index.bin'):
            path = data / name
            path.write_text(f'{cell} {name}\n')
            unstable.append(f'{digest_file(path)}  lib/libpinyin/data/{name}\n')
        (prefix / 'oracle-data-unstable.sha256').write_text(''.join(unstable))
        (prefix / 'oracle-pin.txt').write_text('data_unstable_manifest_sha256=' + digest_file(prefix / 'oracle-data-unstable.sha256') + '\n')
        with self.service.pin.open('a') as pin:
            pin.write(f'data_manifest_sha256_{cell}={digest_file(prefix / "oracle-data.sha256")}\n')

    def source(self, cell, path=None):
        command = 'source "$1" || exit; printf "%s\\0" "$PINYIN_ORACLE_PREFIX" "$OXPINYIN_LIBPINYIN_DATA_DIR" "${OXPINYIN_TKRZW_PREFIX:-}" "${OXPINYIN_TKRZW_LIBRARY:-}" "$PATH" "${LD_LIBRARY_PATH:-}" "${PKG_CONFIG_PATH:-}"'
        return subprocess.run(['bash', '-c', command, 'fixture', str(path or self.service.env_file)],
                              env=dict(self.env, PINYIN_ORACLE_DBM=cell), capture_output=True)

    def old(self, action, args=()):
        # Source only known repository helpers with a synthetic pin and output path.
        script = '''source "$1/tools/cloud/env.sh"
source "$1/tools/oracle/pin.sh"
pin=$2; oracle_root=$3; model_cache=$4; uv_dir=$5; tkrzw_prefix=$6; tkrzw_local=$7
platform=linux/amd64
shift 7
pinval_platform() { pin_platform_field "$pin" "$platform" "$1"; }
log() { printf '%s\\n' "$*" >&2; }
'''
        if action == 'write':
            script += 'cells=("$@"); write_oracle_env\n'
            args = self.service.cells
        else:
            script += 'data_ok "$@"\n'
        return subprocess.run(['bash', '-c', script, 'fixture', str(self.base), str(self.service.pin),
                               str(self.service.oracle_root), str(self.service.model_cache), self.service.uv_dir,
                               str(self.service.tkrzw_prefix), str(int(self.service.tkrzw_local)), *map(str, args)],
                              env=self.env, capture_output=True)

    def test_data_manifest_cases_and_exact_diagnostics(self):
        for cell in provision.CELLS:
            self.assertTrue(self.service.libs_ok(cell))
            self.assertTrue(self.service.data_ok(cell))
        prefix = self.service.oracle_root / 'bdb'
        cases = ['lib/libpinyin/data/' + name for name in
                 ('bigram.db', 'phrase_index.bin', 'pinyin_index.bin', 'punct.bin', 'addon_phrase_index.bin', 'addon_pinyin_index.bin')]
        cases += ['oracle-data-unstable.sha256', 'oracle-pin.txt', 'oracle-data.sha256', 'lib/libpinyin/data/table.conf']
        for name in cases:
            path = prefix / name
            original = path.read_bytes()
            for missing in (False, True):
                with self.subTest(name=name, missing=missing):
                    path.unlink() if missing else path.write_bytes(b'data_unstable_manifest_sha256=bad\n' if name == 'oracle-pin.txt' else original + b'corrupt\n')
                    # Native sha256sum diagnostics use actual fd2; compare via a subprocess.
                    program = ('from pathlib import Path; from tools.dev.provision import Provision; '
                               'import sys; p=Provision(); p.pin=Path(sys.argv[1]); p.platform="linux/amd64"; '
                               'raise SystemExit(0 if p.data_ok("bdb",sys.argv[2]) else 1)')
                    current = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', program,
                                              str(self.service.pin), str(prefix)], cwd=ROOT, env=self.env, capture_output=True)
                    self.assertEqual(current.returncode, 1)
                    if self.base:
                        old = self.old('data', ['bdb', prefix])
                        self.assertEqual((current.returncode, current.stdout, current.stderr),
                                         (old.returncode, old.stdout, old.stderr))
                    path.write_bytes(original)
        self.assertTrue(self.service.data_ok('bdb'))

    def write(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.service.write_oracle_env()
        return err.getvalue()

    def compare_writer(self):
        before = self.service.env_file.read_bytes() if self.service.env_file.exists() else None
        current_error = self.write()
        current = [self.source(cell) for cell in provision.CELLS]
        new_file = self.service.env_file.read_bytes()
        if self.base:
            if before is None:
                self.service.env_file.unlink()
            else:
                self.service.env_file.write_bytes(before)
            old = self.old('write')
            self.assertEqual(old.returncode, 0, old.stderr.decode())
            self.assertEqual(old.stderr.decode(), current_error)
            legacy = [self.source(cell) for cell in provision.CELLS]
            self.assertEqual([(p.returncode, p.stdout, p.stderr) for p in current],
                             [(p.returncode, p.stdout, p.stderr) for p in legacy])
            # Source semantics must match, including historical shell %q paths.
            self.service.env_file.write_bytes(new_file)
        return current

    def test_writer_retains_cells_runtime_quotes_and_another_cache(self):
        self.compare_writer()
        self.service.cells = ['bdb']
        self.service.tkrzw_local = False
        self.compare_writer()
        old_root = self.service.oracle_root
        self.service.cache = self.work / 'another cache'
        (self.service.oracle_root).mkdir(parents=True)
        shutil.copytree(old_root / 'bdb', self.service.oracle_root / 'bdb')
        results = self.compare_writer()
        self.assertIn(str(self.service.oracle_root / 'bdb').encode(), results[0].stdout)
        self.assertIn(str(old_root / 'kc').encode(), results[1].stdout)
        self.assertIn(str(self.work / "cache '$`" / 'tkrzw').encode(), results[2].stdout)
        for cell, name in (('kc', 'lib/libpinyin.so'), ('kc', 'lib/libzhuyin.so'),
                           ('tkrzw', 'oracle-data.sha256'), ('kc', 'lib/libpinyin/data/table.conf'),
                           ('kc', 'lib/libpinyin/data/bigram.db')):
            path = old_root / cell / name
            original = path.read_bytes()
            path.write_bytes(original + b'bad\n')
            results = self.compare_writer()
            self.assertEqual(results[provision.CELLS.index(cell)].returncode, 1)
            path.write_bytes(original)
            self.service.cache = old_root.parent
            self.service.cells = list(provision.CELLS)
            self.compare_writer()
            self.service.cache = self.work / 'another cache'
            self.service.cells = ['bdb']

    def test_never_provisioned_cell_and_atomic_failed_publication(self):
        self.service.cells = ['bdb']
        self.write()
        self.assertEqual(self.source('kc').stderr, b'cell kc was not provisioned by this environment file\n')
        before = self.service.env_file.read_bytes()
        original = provision.run
        def observed(command, **kwargs):
            if command[0] == 'mv':
                self.assertEqual(self.service.env_file.read_bytes(), before)
                self.assertEqual(self.source('bdb', Path(command[1])).returncode, 0)
                raise Finished(75)
            return original(command, **kwargs)
        with mock.patch.object(provision, 'run', side_effect=observed), self.assertRaises(Finished) as error:
            self.write()
        self.assertEqual(error.exception.status, 75)
        self.assertEqual(self.service.env_file.read_bytes(), before)
        self.assertFalse(Path(str(self.service.env_file) + '.new').exists())

    def test_proxy_precedence_private_copy_mode_and_cleanup(self):
        ca = self.work / 'host-ca'
        ca.write_text('synthetic CA\n')
        empty = self.work / 'empty-ca'
        empty.touch()
        self.service.env.update(HTTPS_PROXY='http://127.0.0.1:1', OXPINYIN_CCR_DIR=str(self.work / 'session'),
                                OXPINYIN_SYSTEM_CA_BUNDLES=str(self.work / 'absent'))
        for settings, label in (({'OXPINYIN_PROXY_CA': str(ca)}, '(OXPINYIN_PROXY_CA)'),
                                ({'SSL_CERT_FILE': str(ca), 'CURL_CA_BUNDLE': str(empty)}, 'SSL_CERT_FILE'),
                                ({'CURL_CA_BUNDLE': str(ca)}, 'CURL_CA_BUNDLE'),
                                ({'OXPINYIN_SYSTEM_CA_BUNDLES': str(ca)}, 'system bundle')):
            with self.subTest(label=label):
                base = dict(self.service.env)
                self.service.env.update(settings)
                output = io.StringIO()
                with contextlib.ExitStack() as stack, contextlib.redirect_stderr(output):
                    self.service.proxy_ca(stack)
                    copy = self.service.ca_mount
                    self.assertNotEqual(copy, ca)
                    self.assertEqual(copy.stat().st_mode & 0o777, 0o600)
                    self.assertEqual(copy.read_bytes(), ca.read_bytes())
                    self.assertIn(label, output.getvalue())
                    self.assertNotIn(str(ca) + ':/run/ca.crt', ' '.join(self.service.env_flags))
                self.assertFalse(copy.exists())
                self.service.env = base
        for path in (empty, self.work / 'missing'):
            self.service.env['OXPINYIN_PROXY_CA'] = str(path)
            with contextlib.ExitStack() as stack, contextlib.redirect_stderr(io.StringIO()), self.assertRaises(Finished):
                self.service.proxy_ca(stack)
        self.service.env.pop('OXPINYIN_PROXY_CA')
        with contextlib.ExitStack() as stack, contextlib.redirect_stderr(io.StringIO()), self.assertRaises(Finished):
            self.service.proxy_ca(stack)
        self.service.env.pop('HTTPS_PROXY')
        with contextlib.ExitStack() as stack:
            self.service.proxy_ca(stack)
            self.assertFalse(self.service.custom_ca)

    def test_host_cache_exclusion_loader_and_exact_identity(self):
        caches = [self.service.tkrzw_prefix, self.work / 'cache-b/tkrzw']
        host = self.work / 'host'
        for prefix in [*caches, host]:
            (prefix / 'bin').mkdir(parents=True)
            (prefix / 'lib').mkdir()
            (prefix / 'lib/libtkrzw.so.1').touch()
            util = prefix / 'bin/tkrzw_dbm_util'
            util.write_text('#!/bin/sh\nexit "${FAKE_HOST_STATUS:-0}"\n')
            util.chmod(0o755)
        probe = self.work / 'probe'
        probe.write_text('#!/bin/sh\ntkrzw_dbm_util\n')
        ldd = self.bin / 'ldd'
        ldd.write_text('#!/bin/sh\nprintf "libtkrzw.so.1 => %s (0x1234)\\n" "$FAKE_RESOLVED"\n')
        ldd.chmod(0o755)
        self.write()
        env = dict(self.env, PATH=':'.join(str(p / 'bin') for p in [*caches, host]) + ':' + self.env['PATH'],
                   LD_LIBRARY_PATH=':'.join(str(p / 'lib') for p in [*caches, host]),
                   OXPINYIN_TKRZW_PREFIXES=str(caches[0]), FAKE_RESOLVED=str(host / 'lib/libtkrzw.so.1'))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(provision.probe_host(caches[1], env=env, probe=probe), str(host / 'lib/libtkrzw.so.1'))
            self.assertFalse(provision.probe_host(caches[1], env=dict(env, FAKE_HOST_STATUS='1'), probe=probe))
            self.assertFalse(provision.probe_host(caches[1], env=dict(env, FAKE_RESOLVED=str(caches[0] / 'lib/libtkrzw.so.1')), probe=probe))
        legacy = self.service.env_file.read_text().replace('export OXPINYIN_TKRZW_PREFIXES=', '# old ')
        self.service.env_file.write_text(legacy)
        self.assertEqual(provision.tkrzw_env_prefix(self.service.env_file), str(caches[0]))
        self.service.tkrzw_local = False
        self.service.tkrzw_host_library = str(host / 'lib/libtkrzw.so.1')
        self.write()
        self.assertEqual(provision.tkrzw_env_prefix(self.service.env_file), '')
        self.assertIn(str(host / 'lib/libtkrzw.so.1').encode(), self.source('tkrzw').stdout)

    def test_pool_delegates_and_in_container_never_launches_a_runtime(self):
        self.service.inside = True
        with mock.patch.object(provision, 'run', return_value=19) as child:
            self.assertEqual(self.service.pool_phase('data', ['bdb'], self.work / '{item}.log', ['--dbm', '{item}']), 19)
        command = list(map(str, child.call_args.args[0]))
        self.assertIn('pool', command)
        self.assertNotIn('--container-runtime', command)
        self.assertNotIn('tools.dev', self.service.in_image_command([]))
        self.service.inside = False
        self.service.ctr = 'podman'
        self.service.deps_image = 'synthetic-deps'
        with mock.patch.object(provision, 'run', return_value=0) as child:
            self.service.pool_phase('libraries', ['bdb'], self.work / '{item}.log', [])
        command = list(map(str, child.call_args.args[0]))
        self.assertIn('oxpinyin.setup={token}', command)
        self.assertIn('{cidfile}', command)
        self.assertFalse(any('tools/dev' in part for part in self.service.in_image_command([])))


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux provisioning')
class EntrypointTests(unittest.TestCase):
    """Run real old/new entry points with identical fake host processes."""
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.work = Path(temp.name)
        self.root = self.work / 'tree'
        self.root.mkdir()
        # These are fixture source trees, never reference-image contents.
        shutil.copytree(ROOT / 'tools', self.root / 'tools', ignore=shutil.ignore_patterns('__pycache__'))
        for name in ('.python-version', 'rust-toolchain.toml'):
            shutil.copyfile(ROOT / name, self.root / name)
        self.base = Path(os.environ['U6_BASE']) if os.environ.get('U6_BASE') else None
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.calls = self.work / 'calls'
        program = f'''#!{sys.executable}
import os,pathlib,sys,time
name=pathlib.Path(sys.argv[0]).name; a=sys.argv[1:]
if name=='uv':
    if a==['--version']: print('uv 0.13.1 (synthetic)')
    elif a[:2]==['python','find']: print(sys.executable)
    elif a[:1]==['run']:
        i=a.index('python'); os.execv(sys.executable,[sys.executable,*a[i+1:]])
    else: sys.exit(97)
elif name=='rustup': sys.exit(int(os.environ.get('FAKE_RUST','0')))
elif name=='valgrind': print('valgrind-'+os.environ.get('FAKE_VALGRIND','3.25.1'))
elif name=='podman':
    if a==['--version']: print('podman (synthetic)')
    elif a==['info'] and os.environ.get('FAKE_RUNTIME_READY'): pass
    elif a==['info'] and os.environ.get('FAKE_WAIT'):
        pathlib.Path(os.environ['READY']).touch(); time.sleep(60)
    else: sys.exit(97)
elif name=='findmnt':
    if os.environ.get('FAKE_EXT4')=='unknown': sys.exit(1)
    print('/ overlay' if a[:1]==['--raw'] else 'overlay')
elif name in ('curl','apt-get','sudo'):
    with open(os.environ['CALLS'],'a') as f: f.write(name+' '+ ' '.join(a)+'\\n')
    sys.exit(97)
'''
        for name in ('uv', 'rustup', 'valgrind', 'podman', 'findmnt', 'curl', 'apt-get'):
            path = self.bin / name
            path.write_text(program)
            path.chmod(0o755)
        self.env = dict(PATH=str(self.bin) + ':' + os.defpath, HOME=str(self.work), LC_ALL='C', TMPDIR=str(self.work),
                        UV_CACHE_DIR=str(self.work / 'uv-cache'), OXPINYIN_CLOUD_CACHE=str(self.work / 'cache'),
                        OXPINYIN_ORACLE_ENV=str(self.work / 'oracle.env'), OXPINYIN_CCR_DIR=str(self.work / 'ccr'),
                        OXPINYIN_SYSTEM_CA_BUNDLES=str(self.work / 'no-system-ca'), OXPINYIN_CONTAINER='podman',
                        PINYIN_MODEL_CACHE=str(self.work / 'model20'), CALLS=str(self.calls))
        # Keep host executables isolated; cargo and dump utilities never execute.
        for name in ('cargo', 'db5.3_dump'):
            (self.bin / name).symlink_to('/bin/true')
        self.new_setup = (self.root / 'tools/cloud/setup.sh').read_bytes()

    def run_entry(self, args, old=False):
        setup = self.root / 'tools/cloud/setup.sh'
        if old:
            setup.write_bytes((self.base / 'tools/cloud/setup.sh').read_bytes())
            shutil.copyfile(self.base / 'tools/cloud/env.sh', self.root / 'tools/cloud/env.sh')
        else:
            setup.write_bytes(self.new_setup)
        done = subprocess.run(['bash', str(setup), *args], cwd=self.root, env=self.env,
                              capture_output=True, timeout=25)
        return done

    def normalize(self, data):
        data = re.sub(rb'\[\s*\d+s\]', b'[TIME]', data)
        data = re.sub(rb'at /[^\n]+/ca.crt \(mode', b'at <private>/ca.crt (mode', data)
        return data

    def compare(self, args, expected):
        current = self.run_entry(args)
        self.assertEqual(current.returncode, expected, current.stderr.decode())
        if self.base:
            old = self.run_entry(args, old=True)
            self.assertEqual((current.returncode, self.normalize(current.stdout), self.normalize(current.stderr)),
                             (old.returncode, self.normalize(old.stdout), self.normalize(old.stderr)))
        self.assertFalse(any(path.name == 'ca.crt' for path in self.work.rglob('ca.crt')))
        return current

    def test_usage_options_and_missing_argument_statuses(self):
        for args in (['--unknown'], ['--cells', 'bad'], ['--cells', ''], ['--in-container']):
            with self.subTest(args=args):
                self.compare(args, 2)
        for option in ('--cells', '--jobs', '--cache-dir', '--no-pull', '--in-container'):
            args = ['--python-only', option] + (['bdb' if option == '--cells' else '1'] if option in ('--cells', '--jobs', '--cache-dir') else [])
            self.compare(args, 2)
        for option in ('--cells', '--jobs', '--cache-dir'):
            # Bash-specific unbound-variable diagnostic is deliberately retired;
            # status remains 1, and Python names the missing option.
            done = self.run_entry([option])
            self.assertEqual(done.returncode, 1)
            self.assertEqual(done.stderr, (option + ': requires an argument\n').encode())
            if self.base:
                self.assertEqual(self.run_entry([option], old=True).returncode, 1)

    def test_native_prerequisite_failures_and_python_only_scope(self):
        (self.bin / 'cargo').unlink()
        # /usr/bin may contain cargo on some hosts; fake path must then be masked.
        with mock.patch.object(provision.shutil, 'which', return_value=None), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(Finished):
            provision.Provision(env=self.env).prerequisites()
        self.compare(['--python-only'], 0)
        (self.bin / 'cargo').symlink_to('/bin/true')
        self.env['FAKE_RUST'] = '1'
        self.compare(['--cells', 'bdb'], 1)
        self.env.pop('FAKE_RUST')
        self.compare(['--cells', 'bdb'], 1)  # model access refusal before runtime provisioning
        for settings in ({'HTTPS_PROXY': 'http://127.0.0.1:1'},
                         {'OXPINYIN_PROXY_CA': str(self.work / 'missing')},
                         {'OXPINYIN_PROXY_CA': str(self.work / 'fake-ca')}):
            (self.work / 'fake-ca').write_text('synthetic CA\n')
            self.env.update(settings)
            self.compare(['--cells', 'bdb'], 1)
            for key in settings:
                self.env.pop(key)
        self.env['FAKE_EXT4'] = 'unknown'
        self.compare(['--python-only'], 0)

    def test_cached_full_setup_preserves_result_order_files_and_selection(self):
        cache = self.work / 'cache'
        pin = self.root / 'tools/oracle/oracle-pin.txt'
        fields = pin.read_text()
        for cell in ('bdb', 'kc'):
            prefix = cache / 'oracle' / cell
            data = prefix / 'lib/libpinyin/data'
            data.mkdir(parents=True)
            (prefix / 'lib/pkgconfig').mkdir()
            (prefix / 'lib/pkgconfig/libpinyin.pc').write_text('prefix=/old\n')
            for library in ('libpinyin', 'libzhuyin'):
                path = prefix / 'lib' / (library + '.so')
                path.write_text(cell + ' ' + library + '\n')
                fields = re.sub(r'^' + library + '_sha256_' + cell + r'=.*$',
                                library + '_sha256_' + cell + '=' + digest_file(path), fields, flags=re.M)
            (data / 'table.conf').write_text('synthetic\n')
            (prefix / 'oracle-data.sha256').write_text(digest_file(data / 'table.conf') + '  lib/libpinyin/data/table.conf\n')
            (data / 'bigram.db').write_text('synthetic generated\n')
            (prefix / 'oracle-data-unstable.sha256').write_text(digest_file(data / 'bigram.db') + '  lib/libpinyin/data/bigram.db\n')
            (prefix / 'oracle-pin.txt').write_text('data_unstable_manifest_sha256=' + digest_file(prefix / 'oracle-data-unstable.sha256') + '\n')
            fields = re.sub(r'^data_manifest_sha256_' + cell + r'=.*$',
                            'data_manifest_sha256_' + cell + '=' + digest_file(prefix / 'oracle-data.sha256'), fields, flags=re.M)
        pin.write_text(fields)
        self.env['FAKE_RUNTIME_READY'] = '1'
        (self.bin / 'kchashmgr').symlink_to('/bin/true')
        current = self.compare(['--cells', 'kc bdb'], 0)
        rows = [line for line in current.stdout.decode().splitlines() if 'data manifest OK' in line]
        self.assertEqual([line.split()[0] for line in rows], ['kc', 'bdb'])
        for cell in ('bdb', 'kc'):
            source = subprocess.run(['bash', '-c', 'source "$1"; printf "%s" "$PINYIN_ORACLE_PREFIX"',
                                     'fixture', self.env['OXPINYIN_ORACLE_ENV']],
                                    env=dict(self.env, PINYIN_ORACLE_DBM=cell), capture_output=True)
            self.assertEqual(source.returncode, 0, source.stderr.decode())
            self.assertEqual(source.stdout.decode(), str(cache / 'oracle' / cell))
            self.assertEqual((cache / 'oracle' / cell / 'lib/pkgconfig/libpinyin.pc').read_text(),
                             'prefix=' + str(cache / 'oracle' / cell) + '\n')

    def test_cancellation_reaps_fake_child_and_cleans_private_ca(self):
        # Supply a fake verified model extraction, so cancellation reaches info.
        model = self.work / 'model20'
        extracted = model / 'extracted'
        extracted.mkdir(parents=True)
        for name in ('art culture economy gb_char gbk_char geology history life merged nature opengram people punct science society sport technology'.split()):
            (extracted / (name + '.table')).touch()
        (extracted / 'interpolation2.text').touch()
        (model / 'verified').write_text('sha256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\n')
        ca = self.work / 'synthetic-ca'
        ca.write_text('synthetic CA\n')
        ready = self.work / 'ready'
        self.env.update(FAKE_WAIT='1', READY=str(ready), OXPINYIN_PROXY_CA=str(ca))
        outcomes = []
        for old in ([False, True] if self.base else [False]):
            ready.unlink(missing_ok=True)
            setup = self.root / 'tools/cloud/setup.sh'
            setup.write_bytes((self.base / 'tools/cloud/setup.sh').read_bytes() if old else self.new_setup)
            if old:
                shutil.copyfile(self.base / 'tools/cloud/env.sh', self.root / 'tools/cloud/env.sh')
            child = subprocess.Popen(['bash', str(setup), '--cells', 'bdb'], cwd=self.root, env=self.env,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            self.addCleanup(lambda: child.poll() is not None or os.killpg(child.pid, signal.SIGKILL))
            deadline = time.monotonic() + 10
            while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(.02)
            self.assertTrue(ready.exists(), 'fake runtime did not reach cancellation boundary')
            os.killpg(child.pid, signal.SIGTERM)
            out, err = child.communicate(timeout=10)
            self.assertEqual(child.returncode, 1, err.decode())
            self.assertFalse(any(path.name == 'ca.crt' for path in self.work.rglob('ca.crt')))
            outcomes.append((out, self.normalize(err)))
        # Status and owned cleanup are exact; process-termination diagnostics are
        # Bash-specific and not copied into the Python controller.
        if len(outcomes) == 2:
            self.assertEqual(outcomes[0][0], outcomes[1][0])


if __name__ == '__main__':
    unittest.main()
