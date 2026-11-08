#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Synthetic provisioning fixtures with fake host processes."""
import contextlib
import io
import os
import platform
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
from tools.dev import provision
from tools.oracle.image import digest_file
from tools.dev.native import NativeRefused, Tools
from tools.dev.runners import Finished


def record_prefix(prefix, backend, root=ROOT, pin=None, architecture=None):
    record = provision.images.fields(pin or root / 'tools/oracle/oracle-pin.txt')
    values = {key: record[key] for key in ('libpinyin_commit', 'ibus_libpinyin_commit')}
    values.update(architecture=architecture or platform.machine(), recipe_sha256=provision.images.recipe_hash(root),
                  dbm=provision.backend_cell.BACKEND_FORMATS[provision.backend_cell.CELL_TO_BACKEND[backend]],
                  model_sha256=record['model_archive_sha256'],
                  shared_object_sha256=digest_file(prefix / 'lib/libpinyin.so'),
                  zhuyin_shared_object_sha256=digest_file(prefix / 'lib/libzhuyin.so'))
    (prefix / 'versions.txt').write_text('recorded installed versions\n')
    values['versions_sha256'] = digest_file(prefix / 'versions.txt')
    for file, key in (('oracle-data.sha256', 'data_manifest_sha256'),
                      ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')):
        if (prefix / file).exists():
            values[key] = digest_file(prefix / file)
    (prefix / 'oracle-pin.txt').write_text(''.join(f'{key}={value}\n' for key, value in values.items()))


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux cloud provisioning fixtures')
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
        self.service.pin.write_bytes((ROOT / 'tools/oracle/oracle-pin.txt').read_bytes())
        self.service.platform = 'linux/amd64'
        self.service.cells = ['bdb', 'kc', 'tkrzw']
        self.service.tkrzw_local = True
        self.service.log = lambda value: print(value, file=sys.stderr)
        for cell in provision.CELLS:
            self.fixture(cell)

    def fixture(self, cell):
        prefix = self.service.oracle_root / cell
        data = prefix / 'lib/libpinyin/data'
        data.mkdir(parents=True)
        for library in ('libpinyin', 'libzhuyin'):
            path = prefix / 'lib' / (library + '.so')
            path.write_text(f'{cell} {library}\n')
        (data / 'table.conf').write_text(cell + '\n')
        (prefix / 'oracle-data.sha256').write_text(f'{digest_file(data / "table.conf")}  lib/libpinyin/data/table.conf\n')
        unstable = []
        for name in ('bigram.db', 'phrase_index.bin', 'pinyin_index.bin', 'punct.bin', 'addon_phrase_index.bin', 'addon_pinyin_index.bin'):
            path = data / name
            path.write_text(f'{cell} {name}\n')
            unstable.append(f'{digest_file(path)}  lib/libpinyin/data/{name}\n')
        (prefix / 'oracle-data-unstable.sha256').write_text(''.join(unstable))
        record_prefix(prefix, cell, pin=self.service.pin, architecture='x86_64')

    def test_library_provenance_refuses_old_pin_recipe_versions_and_damaged_bytes(self):
        prefix = self.service.oracle_root / 'bdb'
        pin = prefix / 'oracle-pin.txt'
        original = pin.read_text()
        for key in ('libpinyin_commit', 'ibus_libpinyin_commit', 'recipe_sha256', 'architecture'):
            pin.write_text(re.sub(r'^' + key + '=.*$', key + '=different', original, flags=re.M))
            self.assertFalse(self.service.libs_ok('bdb'), key)
            pin.write_text(original)
        for path in (prefix / 'lib/libpinyin.so', prefix / 'lib/libzhuyin.so', prefix / 'versions.txt'):
            data = path.read_bytes()
            path.write_bytes(data + b'damaged')
            self.assertFalse(self.service.libs_ok('bdb'), str(path))
            path.write_bytes(data)
        self.assertTrue(self.service.libs_ok('bdb'))

    def source(self, cell, path=None):
        command = 'source "$1" || exit; printf "%s\\0" "$PINYIN_ORACLE_PREFIX" "$OXPINYIN_LIBPINYIN_DATA_DIR" "${OXPINYIN_TKRZW_PREFIX:-}" "${OXPINYIN_TKRZW_LIBRARY:-}" "$PATH" "${LD_LIBRARY_PATH:-}" "${PKG_CONFIG_PATH:-}"'
        return subprocess.run(['bash', '-c', command, 'fixture', str(path or self.service.env_file)],
                              env=dict(self.env, PINYIN_ORACLE_DBM=cell), capture_output=True)

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
                    path.write_bytes(original)
        self.assertTrue(self.service.data_ok('bdb'))

    def write(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.service.write_oracle_env()
        return err.getvalue()

    def write_and_source(self):
        self.write()
        return [self.source(cell) for cell in provision.CELLS]

    def test_writer_retains_cells_runtime_quotes_and_another_cache(self):
        self.write_and_source()
        self.service.cells = ['bdb']
        self.service.tkrzw_local = False
        self.write_and_source()
        old_root = self.service.oracle_root
        self.service.cache = self.work / 'another cache'
        (self.service.oracle_root).mkdir(parents=True)
        shutil.copytree(old_root / 'bdb', self.service.oracle_root / 'bdb')
        results = self.write_and_source()
        self.assertIn(str(self.service.oracle_root / 'bdb').encode(), results[0].stdout)
        self.assertIn(str(old_root / 'kc').encode(), results[1].stdout)
        self.assertIn(str(self.work / "cache '$`" / 'tkrzw').encode(), results[2].stdout)
        for cell, name in (('kc', 'lib/libpinyin.so'), ('kc', 'lib/libzhuyin.so'),
                           ('tkrzw', 'oracle-data.sha256'), ('kc', 'lib/libpinyin/data/table.conf'),
                           ('kc', 'lib/libpinyin/data/bigram.db')):
            path = old_root / cell / name
            original = path.read_bytes()
            path.write_bytes(original + b'bad\n')
            results = self.write_and_source()
            self.assertEqual(results[provision.CELLS.index(cell)].returncode, 1)
            path.write_bytes(original)
            self.service.cache = old_root.parent
            self.service.cells = list(provision.CELLS)
            self.write_and_source()
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
        ubuntu, debian = self.work / 'os-release-ubuntu', self.work / 'os-release-debian'
        ubuntu.write_text('NAME="Ubuntu"\nID=ubuntu\nID_LIKE=debian\n')
        debian.write_text('NAME="Debian GNU/Linux"\nID=debian\n')
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(provision.probe_host(caches[1], env=env, probe=probe, os_release=ubuntu),
                             str(host / 'lib/libtkrzw.so.1'))
            self.assertFalse(provision.probe_host(caches[1], env=dict(env, FAKE_HOST_STATUS='1'), probe=probe,
                                                  os_release=ubuntu))
            self.assertFalse(provision.probe_host(caches[1], env=dict(env, FAKE_RESOLVED=str(caches[0] / 'lib/libtkrzw.so.1')),
                                                  probe=probe, os_release=ubuntu))
            # Off Ubuntu the distribution package needs no distro-probe.sh: a failing probe is never run,
            # and the loader checks still refuse a cached library.
            self.assertEqual(provision.probe_host(caches[1], env=dict(env, FAKE_HOST_STATUS='1'), probe=probe,
                                                  os_release=debian), str(host / 'lib/libtkrzw.so.1'))
            self.assertFalse(provision.probe_host(caches[1], env=dict(env, FAKE_RESOLVED=str(caches[0] / 'lib/libtkrzw.so.1')),
                                                  probe=probe, os_release=debian))
        legacy = self.service.env_file.read_text().replace('export OXPINYIN_TKRZW_PREFIXES=', '# old ')
        self.service.env_file.write_text(legacy)
        self.assertEqual(provision.tkrzw_env_prefix(self.service.env_file), str(caches[0]))
        self.service.tkrzw_local = False
        self.service.tkrzw_host_library = str(host / 'lib/libtkrzw.so.1')
        self.write()
        self.assertEqual(provision.tkrzw_env_prefix(self.service.env_file), '')
        self.assertIn(str(host / 'lib/libtkrzw.so.1').encode(), self.source('tkrzw').stdout)

    def test_ubuntu_hosts_are_named_by_os_release(self):
        for text, expected in (('ID=ubuntu\n', True), ('ID="ubuntu"\n', True), ('ID=linuxmint\nID_LIKE="ubuntu debian"\n', True),
                               ('ID=debian\n', False), ('ID=fedora\nID_LIKE="rhel centos"\n', False), ('', False)):
            with self.subTest(text=text):
                path = self.work / 'os-release'
                path.write_text(text)
                self.assertIs(provision.ubuntu_host(path), expected)
        absent = self.work / 'absent-os-release'
        self.assertFalse(provision.ubuntu_host(absent, absent))

    def test_os_release_falls_back_to_usr_lib(self):
        fallback = self.work / 'usr-lib-os-release'
        fallback.write_text('ID=ubuntu\n')
        absent = self.work / 'absent-os-release'
        self.assertTrue(provision.ubuntu_host(absent, fallback))
        self.assertEqual(provision.os_release_record(absent, fallback), {'ID': 'ubuntu'})
        first = self.work / 'etc-os-release'
        first.write_text('ID=debian\n')
        self.assertFalse(provision.ubuntu_host(first, fallback))
        self.assertIsNone(provision.os_release_record(absent, absent))

    def test_a_linux_host_without_os_release_is_not_certified(self):
        absent = self.work / 'absent-os-release'
        with (mock.patch.object(provision.platform, 'system', return_value='Linux'),
              mock.patch.object(provision, 'run') as run, contextlib.redirect_stderr(io.StringIO()) as error):
            self.assertEqual(provision.probe_host('', env={'PATH': ''}, os_release=absent, os_release_fallback=absent), '')
        run.assert_not_called()
        self.assertIn(f'host check: neither {absent} nor {absent} can be read, so the host cannot be named; '
                      'no healthy host libtkrzw was certified', error.getvalue())

    def test_every_empty_host_probe_result_reports_its_reason_on_stderr(self):
        ubuntu = self.work / 'os-release-ubuntu'
        ubuntu.write_text('ID=ubuntu\n')
        library = self.work / 'host-libtkrzw.so.1'
        library.touch()
        cached = self.service.tkrzw_prefix / 'lib/libtkrzw.so.1'
        cached.parent.mkdir(parents=True)
        cached.touch()
        alias = self.work / 'host-library-link'
        alias.symlink_to(cached)
        util = str(self.work / 'tkrzw_dbm_util')
        cases = (
            ('failed distro probe', 7, util, str(library), None,
             'host distro-probe.sh failed (status 7)'),
            ('missing utility', 0, None, str(library), None,
             'tkrzw_dbm_util is not on the host PATH after excluding cached runtimes'),
            ('failed loader', 0, util, str(library), Finished(126),
             'the host utility loader check failed (status 126)'),
            ('unresolved library', 0, util, '', None,
             'the host utility did not resolve libtkrzw'),
            ('missing library file', 0, util, str(self.work / 'absent-library'), None,
             'resolved host libtkrzw is not a file:'),
            ('excluded cache', 0, util, str(cached), None,
             'resolved libtkrzw belongs to an excluded cache:'),
            ('cache through symlink', 0, util, str(alias), None,
             'resolved host libtkrzw points into an excluded cache:'),
        )
        for name, status, executable, resolved, error, reason in cases:
            output, diagnostic = io.StringIO(), io.StringIO()
            with self.subTest(path=name), \
                    mock.patch.object(provision, 'run', return_value=status), \
                    mock.patch.object(provision.shutil, 'which', return_value=executable), \
                    mock.patch.object(provision, 'resolve_tkrzw', return_value=resolved,
                                      side_effect=error), \
                    mock.patch.object(provision, 'text', return_value='Linux'), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(diagnostic):
                self.assertEqual(provision.probe_host(self.service.tkrzw_prefix, env=self.env, os_release=ubuntu), '')
            self.assertIn(reason, diagnostic.getvalue())
            self.assertIn('no healthy host libtkrzw was certified', diagnostic.getvalue())
            self.assertNotIn(reason, output.getvalue())

    def test_uncertified_host_names_the_outcome_and_keeps_status_one(self):
        with mock.patch.object(provision, 'probe_host', return_value=''):
            with self.assertRaisesRegex(NativeRefused, '^no healthy host libtkrzw was certified$'):
                Tools(environment=self.env).tkrzw_host()
            self.assertEqual(provision.main(['probe-host']), 1)

    def test_shell_quoting_round_trips_without_evaluating_values(self):
        for value in ("", "plain/a-b.c_+=,:%@", "quotes '\" and $dollars`", "a\nb\tc\r\x01\x7f", "~#&|;()<>!?[]{}*"):
            for always in (False, True):
                with self.subTest(value=value, always=always):
                    decoded = subprocess.check_output(['bash', '-c', 'value=' + provision.quote(value, always=always) + '; printf "%s" "$value"'], env=self.env).decode()
                    self.assertEqual(decoded, value)

    def test_ext4_reports_available_absent_overmounted_inaccessible_and_unknown(self):
        mount = self.work / 'ext4 mount'
        mount.mkdir()
        findmnt = self.bin / 'findmnt'
        findmnt.write_text(f"""#!{sys.executable}
import os,sys
mode=os.environ['FAKE_EXT4']
if mode=='unknown': sys.exit(1)
if sys.argv[1]=='--raw':
    print('/ overlay')
    if mode!='absent': print(os.environ['FAKE_MOUNT'].replace(' ', r'\\x20')+' ext4')
else:
    print('ext4' if mode=='available' and sys.argv[-1]==os.environ['FAKE_MOUNT'] else 'overlay')
""")
        findmnt.chmod(0o755)
        self.service.env['FAKE_MOUNT'] = str(mount)
        for mode, expected in (('available', f'ext4 path (readable, writable, searchable): {mount}'),
                               ('absent', 'ext4 paths: none accessible'),
                               ('overmounted', 'ext4 paths: none accessible'),
                               ('unknown', 'ext4 paths: unknown (mount detection failed)')):
            self.service.env['FAKE_EXT4'] = mode
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.service.report_ext4()
            self.assertIn(expected, output.getvalue())
            self.assertIn('does not verify all measured files are on ext4', output.getvalue())
        mount.rmdir()
        self.service.env['FAKE_EXT4'] = 'available'
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.service.report_ext4()
        self.assertIn('ext4 paths: none accessible', output.getvalue())
        with mock.patch.object(self.service, 'which', return_value=None), contextlib.redirect_stdout(output):
            self.service.report_ext4()
        self.assertIn('ext4 paths: unknown (findmnt unavailable)', output.getvalue())

    def test_pool_delegates_with_owned_container_teardown(self):
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
    """Run the setup entry point with fake host processes."""
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
        # A multi-cell provision resolves the default backend from the store manifest.
        (self.root / 'crates/oxpinyin-store').mkdir(parents=True)
        shutil.copyfile(ROOT / 'crates/oxpinyin-store/Cargo.toml', self.root / 'crates/oxpinyin-store/Cargo.toml')
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.calls = self.work / 'calls'
        program = f'''#!{sys.executable}
import os,pathlib,sys,time
name=pathlib.Path(sys.argv[0]).name; a=sys.argv[1:]
if name=='uv':
    if a==['--version']: print('uv '+os.environ.get('FAKE_UV_VERSION','0.13.1')+' (synthetic)')
    elif a[:2]==['python','find']:
        if os.environ.get('FAKE_PYTHON_FAILURE'): sys.exit(1)
        print(sys.executable)
    elif a[:2]==['python','install']: sys.exit(1 if os.environ.get('FAKE_PYTHON_FAILURE') else 0)
    elif a[:1]==['run']:
        i=a.index('python'); os.execv(sys.executable,[sys.executable,*a[i+1:]])
    else: sys.exit(97)
elif name=='rustup': sys.exit(int(os.environ.get('FAKE_RUST','0')))
elif name=='valgrind':
    if os.environ.get('FAKE_VALGRIND_MISSING'): sys.exit(1)
    print('valgrind-'+os.environ.get('FAKE_VALGRIND','3.25.1'))
elif name=='podman':
    if a==['--version']: print('podman (synthetic)')
    elif a==['info'] and os.environ.get('FAKE_RUNTIME_READY'): pass
    elif a[:2]==['image','inspect'] and os.environ.get('FAKE_POOL'): pass
    elif a[:1]==['run'] and os.environ.get('FAKE_POOL'):
        volume=next(v for v in a if ':/opt/pinyin-oracle:z' in v).split(':/opt/pinyin-oracle:z')[0]
        cell=a[a.index('--dbm')+1]; prefix=pathlib.Path(volume)/cell/'lib'; prefix.mkdir(parents=True,exist_ok=True)
        for lib in ('libpinyin','libzhuyin'): (prefix/(lib+'.so')).write_text('synthetic wrong library\\n')
    elif a==['info'] and os.environ.get('FAKE_WAIT'):
        pathlib.Path(os.environ['READY']).touch(); time.sleep(60)
    else: sys.exit(97)
elif name=='wslc':
    if a==['--version']: print('wslc 3.0.3.0 (synthetic)\\nCopyright (synthetic)')
    else: sys.exit(97)
elif name=='findmnt':
    if os.environ.get('FAKE_EXT4')=='unknown': sys.exit(1)
    print('/ overlay' if a[:1]==['--raw'] else 'overlay')
elif name in ('curl','apt-get','sudo'):
    with open(os.environ['CALLS'],'a') as f: f.write(name+' '+ ' '.join(a)+'\\n')
    sys.exit(97)
'''
        for name in ('uv', 'rustup', 'valgrind', 'podman', 'wslc', 'findmnt', 'curl', 'apt-get', 'sudo'):
            path = self.bin / name
            path.write_text(program)
            path.chmod(0o755)
        self.env = dict(PATH=str(self.bin) + ':' + os.defpath, HOME=str(self.work), LC_ALL='C', TMPDIR=str(self.work),
                        UV_CACHE_DIR=str(self.work / 'uv-cache'), OXPINYIN_CLOUD_CACHE=str(self.work / 'cache'),
                        OXPINYIN_ORACLE_ENV=str(self.work / 'oracle.env'), OXPINYIN_CCR_DIR=str(self.work / 'ccr'),
                        OXPINYIN_SYSTEM_CA_BUNDLES=str(self.work / 'no-system-ca'), OXPINYIN_CONTAINER='podman',
                        PINYIN_MODEL_CACHE=str(self.work / 'model20'), CALLS=str(self.calls))
        # Keep host executables isolated; cargo and dump utilities never execute.
        (self.bin / 'db5.3_dump').symlink_to('/bin/true')
        cargo = self.bin / 'cargo'
        cargo.write_text('#!/bin/sh\nexit 0\n')
        cargo.chmod(0o755)

    def run_entry(self, args):
        return subprocess.run(['bash', str(self.root / 'tools/cloud/setup.sh'), *args],
                              cwd=self.root, env=self.env, capture_output=True, timeout=25)

    def check_entry(self, args, expected):
        current = self.run_entry(args)
        self.assertEqual(current.returncode, expected, current.stderr.decode())
        self.assertFalse(any(path.name == 'ca.crt' for path in self.work.rglob('ca.crt')))
        return current

    def test_usage_options_and_missing_argument_statuses(self):
        for args in (['--unknown'], ['--cells', 'bad'], ['--cells', '']):
            with self.subTest(args=args):
                self.check_entry(args, 2)
        for option in ('--cells', '--jobs', '--cache-dir', '--no-pull'):
            args = ['--python-only', option] + (['bdb' if option == '--cells' else '1'] if option in ('--cells', '--jobs', '--cache-dir') else [])
            self.check_entry(args, 2)
        for option in ('--cells', '--jobs', '--cache-dir'):
            # Bash-specific unbound-variable diagnostic is deliberately retired;
            # status remains 1, and Python names the missing option.
            done = self.run_entry([option])
            self.assertEqual(done.returncode, 1)
            self.assertEqual(done.stderr, (option + ': requires an argument\n').encode())

    def test_uv_bootstrap_semver_and_install_failures(self):
        for version in ('0.13.1', '0.14.2', '0.13.1+build.7', '0.14.0-beta.1', '0.13.2-rc.1+build.7'):
            with self.subTest(version=version):
                self.env['FAKE_UV_VERSION'] = version
                self.check_entry(['--python-only'], 0)
        for version in ('0.13.0', '0.13.1-beta.1', '0.13.1-rc.1+build.7', '0.13.1-0', '0.12.9-rc.1'):
            with self.subTest(version=version):
                self.env['FAKE_UV_VERSION'] = version
                result = self.check_entry(['--python-only'], 1)
                self.assertIn(b'FAIL: uv download failed', result.stderr)
        self.env.pop('FAKE_UV_VERSION')
        destination = self.work / 'regular-file'
        destination.touch()
        self.env['UV_INSTALL_DIR'] = str(destination)
        self.assertIn(b'uv installation directory is not writable', self.check_entry(['--python-only'], 1).stderr)
        self.env.pop('UV_INSTALL_DIR')
        self.env['FAKE_PYTHON_FAILURE'] = '1'
        self.assertIn(b'FAIL: managed Python provisioning failed', self.check_entry(['--python-only'], 1).stderr)
        self.env.pop('FAKE_PYTHON_FAILURE')

    def test_valgrind_minimum_and_install_retry(self):
        for version in ('3.22.0', '3.27.1', '3.22.0.GIT', '3.21.0'):
            with self.subTest(version=version):
                self.env['FAKE_VALGRIND'] = version
                result = self.check_entry(['--cells', 'bdb'], 1)
                if version == '3.21.0':
                    self.assertIn(b'FAIL: could not install valgrind with apt-get', result.stderr)
                else:
                    self.assertIn(b'FAIL: model20 is unavailable', result.stderr)
        self.env.pop('FAKE_VALGRIND')
        self.env['FAKE_VALGRIND_MISSING'] = '1'
        result = self.check_entry(['--cells', 'bdb'], 1)
        self.assertIn(b'FAIL: could not install valgrind with apt-get', result.stderr)
        self.env.pop('FAKE_VALGRIND_MISSING')

    def test_native_prerequisite_failures_and_python_only_scope(self):
        (self.bin / 'cargo').unlink()
        # /usr/bin may contain cargo on some hosts; fake path must then be masked.
        with mock.patch.object(provision.shutil, 'which', return_value=None), contextlib.redirect_stderr(io.StringIO()), self.assertRaises(Finished):
            provision.Provision(env=self.env).prerequisites()
        self.check_entry(['--python-only'], 0)
        (self.bin / 'cargo').symlink_to('/bin/true')
        self.env['FAKE_RUST'] = '1'
        self.check_entry(['--cells', 'bdb'], 1)
        self.env.pop('FAKE_RUST')
        self.check_entry(['--cells', 'bdb'], 1)  # model access refusal before runtime provisioning
        for settings in ({'HTTPS_PROXY': 'http://127.0.0.1:1'},
                         {'OXPINYIN_PROXY_CA': str(self.work / 'missing')},
                         {'OXPINYIN_PROXY_CA': str(self.work / 'fake-ca')}):
            (self.work / 'fake-ca').write_text('synthetic CA\n')
            self.env.update(settings)
            self.check_entry(['--cells', 'bdb'], 1)
            for key in settings:
                self.env.pop(key)
        self.env['FAKE_EXT4'] = 'unknown'
        self.check_entry(['--python-only'], 0)

    def test_wslc_is_refused_before_any_runtime_command(self):
        self.env['OXPINYIN_CONTAINER'] = 'wslc'
        current = self.check_entry(['--cells', 'bdb'], 1)
        self.assertIn(b'wslc (WSL containers) is not supported', current.stderr)
        self.assertFalse(self.calls.exists())

    def test_cached_full_setup_preserves_result_order_files_and_selection(self):
        cache = self.work / 'cache'
        pin = self.root / 'tools/oracle/oracle-pin.txt'
        for cell in ('bdb', 'kc'):
            prefix = cache / 'oracle' / cell
            data = prefix / 'lib/libpinyin/data'
            data.mkdir(parents=True)
            (prefix / 'lib/pkgconfig').mkdir()
            (prefix / 'lib/pkgconfig/libpinyin.pc').write_text('prefix=/old\n')
            for library in ('libpinyin', 'libzhuyin'):
                path = prefix / 'lib' / (library + '.so')
                path.write_text(cell + ' ' + library + '\n')
            (data / 'table.conf').write_text('synthetic\n')
            (prefix / 'oracle-data.sha256').write_text(digest_file(data / 'table.conf') + '  lib/libpinyin/data/table.conf\n')
            (data / 'bigram.db').write_text('synthetic generated\n')
            (prefix / 'oracle-data-unstable.sha256').write_text(digest_file(data / 'bigram.db') + '  lib/libpinyin/data/bigram.db\n')
            record_prefix(prefix, cell, self.root)
        self.env['FAKE_RUNTIME_READY'] = '1'
        (self.bin / 'kchashmgr').symlink_to('/bin/true')
        current = self.check_entry(['--cells', 'kc bdb'], 0)
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

    def verified_model(self):
        model = self.work / 'model20'
        extracted = model / 'extracted'
        extracted.mkdir(parents=True, exist_ok=True)
        for name in ('art culture economy gb_char gbk_char geology history life merged nature opengram people punct science society sport technology'.split()):
            (extracted / (name + '.table')).touch()
        (extracted / 'interpolation2.text').touch()
        (model / 'verified').write_text('sha256=59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155\n')

    def test_cold_no_pull_unrecorded_library_is_rejected(self):
        self.verified_model()
        self.env.update(FAKE_POOL='1', FAKE_RUNTIME_READY='1')
        done = self.run_entry(['--cells', 'bdb', '--no-pull', '--jobs', '1'])
        self.assertEqual(done.returncode, 1, done.stderr.decode())
        self.assertIn(b'stale pin/recipe or damaged build provenance', done.stderr)
        for library in ('libpinyin', 'libzhuyin'):
            self.assertTrue((self.work / 'cache/oracle/bdb/lib' / (library + '.so')).is_file())

    def test_cancellation_reaps_fake_child_and_cleans_private_ca(self):
        # Supply a fake verified model extraction, so cancellation reaches info.
        self.verified_model()
        ca = self.work / 'synthetic-ca'
        ca.write_text('synthetic CA\n')
        ready = self.work / 'ready'
        self.env.update(FAKE_WAIT='1', READY=str(ready))
        for custom in (True, False):
            if custom:
                self.env['OXPINYIN_PROXY_CA'] = str(ca)
            else:
                self.env.pop('OXPINYIN_PROXY_CA', None)
            ready.unlink(missing_ok=True)
            setup = self.root / 'tools/cloud/setup.sh'
            child = subprocess.Popen(['bash', str(setup), '--cells', 'bdb'], cwd=self.root, env=self.env,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            self.addCleanup(lambda: child.poll() is not None or os.killpg(child.pid, signal.SIGKILL))
            deadline = time.monotonic() + 10
            while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(.02)
            self.assertTrue(ready.exists(), 'fake runtime did not reach cancellation boundary')
            os.killpg(child.pid, signal.SIGTERM)
            _, err = child.communicate(timeout=10)
            status = 128 - child.returncode if child.returncode < 0 else child.returncode
            self.assertEqual(status, 1 if custom else 143, err.decode())
            self.assertFalse(any(path.name == 'ca.crt' for path in self.work.rglob('ca.crt')))


class InImageCommandTests(unittest.TestCase):
    def test_deps_image_run_mounts_the_bash_floor_next_to_the_build_script(self):
        # build-oracle.sh sources ../lib/bash-floor.sh relative to /usr/local/bin; a deps image
        # published before the floor existed does not carry it.
        run = object.__new__(provision.Provision)
        run.ctr, run.platform = 'podman', 'linux/amd64'
        run.net_flags, run.env_flags = [], []
        run.root, run.pin = Path('/checkout'), Path('/checkout/tools/oracle/oracle-pin.txt')
        run.cache, run.model_cache, run.deps_image = Path('/cache'), Path('/model'), 'deps:tag'
        command = run.in_image_command(['--libs-only'])
        self.assertIn('/checkout/tools/oracle/build-oracle.sh:/usr/local/bin/build-oracle.sh:ro,z', command)
        self.assertIn('/checkout/tools/lib/bash-floor.sh:/usr/local/lib/bash-floor.sh:ro,z', command)


if __name__ == '__main__':
    unittest.main()
