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


def copy_entrypoint_tree(root):
    """Copy the provisioning sources and their repository data dependencies."""
    shutil.copytree(ROOT / 'tools', root / 'tools', ignore=shutil.ignore_patterns('__pycache__'))
    pin = Path('vcpkg-overlays/tkrzw/pin.json')
    (root / pin).parent.mkdir(parents=True)
    shutil.copyfile(ROOT / pin, root / pin)
    for name in ('.python-version', 'rust-toolchain.toml', 'crates/oxpinyin-store/Cargo.toml'):
        destination = root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(ROOT / name, destination)


class EntrypointFixtureTests(unittest.TestCase):
    def test_copied_tree_imports_provision_with_the_shared_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            copy_entrypoint_tree(root)
            result = subprocess.run(
                [sys.executable, '-I', '-X', 'lazy_imports=normal', '-c',
                 'import sys; sys.path.insert(0, sys.argv[1]); '
                 'from tools.dev.provision import TKRZW_REV; print(TKRZW_REV)', str(root)],
                cwd=root, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), provision.TKRZW_REV)


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


if sys.platform.startswith('linux'):
    class ProvisionTests(unittest.TestCase):
        def setUp(self):
            temporary = tempfile.TemporaryDirectory()
            self.addCleanup(temporary.cleanup)
            self.work = Path(temporary.name)
            self.bin = self.work / 'bin'
            self.bin.mkdir()
            # These tests never inspect a real environment/profile/CA or inherited secrets.
            self.env = dict(PATH=str(self.bin) + ':' + os.defpath, HOME=str(self.work), LC_ALL='C',
                            TMPDIR=str(self.work), OXPINYIN_ORACLE_ENV=str(self.work / 'oracle.env'),
                            OXPINYIN_CACHE=str(self.work / "root '$`"))
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
            self.assertIn(str(self.service.tkrzw_prefix).encode(), results[2].stdout)
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


if sys.platform.startswith('linux'):
    class EntrypointTests(unittest.TestCase):
        """Run the setup entry point with fake host processes."""
        def setUp(self):
            temp = tempfile.TemporaryDirectory()
            self.addCleanup(temp.cleanup)
            self.work = Path(temp.name)
            self.root = self.work / 'tree'
            self.root.mkdir()
            # These are fixture source trees, never reference-image contents.
            copy_entrypoint_tree(self.root)
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
elif name=='container':
    if a==['--version']: print('container CLI version 0.0.0 (synthetic)')
    else: sys.exit(97)
elif name=='findmnt':
    if os.environ.get('FAKE_EXT4')=='unknown': sys.exit(1)
    print('/ overlay' if a[:1]==['--raw'] else 'overlay')
elif name in ('curl','apt-get','sudo'):
    with open(os.environ['CALLS'],'a') as f: f.write(name+' '+ ' '.join(a)+'\\n')
    sys.exit(97)
    '''
            for name in ('uv', 'rustup', 'valgrind', 'podman', 'wslc', 'container', 'findmnt', 'curl', 'apt-get', 'sudo'):
                path = self.bin / name
                path.write_text(program)
                path.chmod(0o755)
            self.env = dict(PATH=str(self.bin) + ':' + os.defpath, HOME=str(self.work), LC_ALL='C', TMPDIR=str(self.work),
                            UV_CACHE_DIR=str(self.work / 'uv-cache'), OXPINYIN_CACHE=str(self.work / 'cache'),
                            OXPINYIN_ORACLE_ENV=str(self.work / 'oracle.env'), OXPINYIN_CCR_DIR=str(self.work / 'ccr'),
                            OXPINYIN_SYSTEM_CA_BUNDLES=str(self.work / 'no-system-ca'), OXPINYIN_CONTAINER='podman',
                            CALLS=str(self.calls))
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
            for option in ('--cells', '--jobs', '--no-pull'):
                args = ['--python-only', option] + (['bdb' if option == '--cells' else '1'] if option in ('--cells', '--jobs') else [])
                self.check_entry(args, 2)
            # The host takes OXPINYIN_CACHE; --cache-dir exists only at the performance image's mount.
            self.check_entry(['--cache-dir', str(self.work / 'named')], 2)
            for option in ('--cells', '--jobs'):
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

        def test_wslc_and_apple_container_are_provisioned_through_the_adapter(self):
            # Neither is refused: each reaches its own readiness command, which these fakes fail.
            self.verified_model()
            for runtime in ('wslc', 'container'):
                with self.subTest(runtime=runtime):
                    self.env['OXPINYIN_CONTAINER'] = runtime
                    current = self.check_entry(['--cells', 'bdb'], 1)
                    self.assertNotIn(b'not supported', current.stderr)
                    self.assertIn(f'FAIL: {runtime} info failed; there is no daemon to start for it'.encode(), current.stderr)

        def test_cached_full_setup_preserves_result_order_files_and_selection(self):
            cache = provision.images.provision_cache(self.env, self.root)
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
            model = provision.images.model_cache(self.env)
            extracted = model / 'extracted'
            extracted.mkdir(parents=True, exist_ok=True)
            for name in ('art culture economy gb_char gbk_char geology history life merged nature opengram people punct science society sport technology'.split()):
                (extracted / (name + '.table')).touch()
            (extracted / 'interpolation2.text').touch()
            (model / 'verified').write_text(f'sha256={provision.model.archive_sha256()}\n')

        def test_cold_no_pull_unrecorded_library_is_rejected(self):
            self.verified_model()
            self.env.update(FAKE_POOL='1', FAKE_RUNTIME_READY='1')
            done = self.run_entry(['--cells', 'bdb', '--no-pull', '--jobs', '1'])
            self.assertEqual(done.returncode, 1, done.stderr.decode())
            self.assertIn(b'stale pin/recipe or damaged build provenance', done.stderr)
            cache = provision.images.provision_cache(self.env, self.root)
            for library in ('libpinyin', 'libzhuyin'):
                self.assertTrue((cache / 'oracle/bdb/lib' / (library + '.so')).is_file())

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


class RuntimeProvisionTests(unittest.TestCase):
    """The command each runtime gets, and the host-only steps per OS; no container runs."""
    def setUp(self):
        # `container` names Apple's CLI only on macOS; on Windows it is wslc's alias.
        patcher = mock.patch.object(provision.images, 'host_system', return_value='Darwin')
        patcher.start()
        self.addCleanup(patcher.stop)
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.work = Path(directory.name)
        self.calls = []

    def service(self, ctr, host='Linux'):
        service = provision.Provision(root=ROOT, env={'PATH': '', 'OXPINYIN_CACHE': str(self.work / 'root')})
        service.host, service.ctr, service.platform = host, ctr, 'linux/amd64'
        service.cache = self.work / 'cache'
        service.cache.mkdir(exist_ok=True)
        service.model_cache = self.work / 'model20'
        service.deps_image, service.base_image = 'deps:tag', 'debian:testing'
        service.published, service.published_deps = 'pub:tag', 'pub:tag-deps'
        service.proxy, service.custom_ca, service.env_flags = '', False, []
        service.cells = ['tkrzw']
        service.log = lambda message: self.calls.append(['log', message])
        return service

    def runner(self, statuses=None):
        statuses = statuses or {}

        def fake(command, **kwargs):
            command = [str(part) for part in command]
            self.calls.append(command)
            return next((status for key, status in statuses.items() if key in command), 0)
        return fake

    def commands(self):
        return [call for call in self.calls if call[0] != 'log']

    def test_the_build_run_mounts_per_runtime(self):
        commands = {ctr: self.service(ctr).in_image_command(['--libs-only']) for ctr in ('docker', 'podman', 'wslc', 'container')}
        recipe = f'{ROOT}:/opt/oracle-recipe'
        for ctr in ('docker', 'podman'):
            self.assertIn(recipe + ':ro,z', commands[ctr])
            self.assertEqual(commands[ctr][commands[ctr].index('--platform') + 1], 'linux/amd64')
            self.assertIn('--network', commands[ctr])
        self.assertIn(recipe + ':ro', commands['wslc'])
        self.assertNotIn('--platform', commands['wslc'])
        self.assertNotIn('--network', commands['wslc'])
        self.assertIn(f'type=bind,source={ROOT},target=/opt/oracle-recipe,readonly', commands['container'])
        self.assertNotIn('--network', commands['container'])
        for ctr in ('wslc', 'container'):
            self.assertFalse(any(str(part).endswith(('z', ',z')) and ':' in str(part) for part in commands[ctr]), ctr)
        for ctr, command in commands.items():
            self.assertEqual(command[-3:], ['python3', '/opt/oracle-recipe/tools/oracle/build.py', '--libs-only'], ctr)

    def test_runtime_readiness_asks_each_runtime_its_own_way(self):
        for ctr, expected in (('docker', ['docker', 'info']), ('podman', ['podman', 'info']),
                              ('wslc', ['wslc', 'info']), ('container', ['container', 'system', 'status'])):
            with self.subTest(ctr=ctr):
                self.calls.clear()
                with mock.patch.object(provision, 'run', self.runner()):
                    service = self.service(ctr)
                    service.runtime_ready()
                    service.runtime_ready()
                self.assertEqual(self.commands(), [expected])

    def test_docker_without_a_daemon_starts_dockerd_only_on_linux(self):
        for host in ('Darwin', 'Windows'):
            with (self.subTest(host=host), mock.patch.object(provision, 'run', self.runner({'info': 1})),
                  contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(Finished)):
                self.service('docker', host).runtime_ready()
            self.assertIn('start Docker Desktop or the Docker daemon (dockerd is started only on Linux)', error.getvalue())
        with (mock.patch.object(provision, 'run', self.runner({'info': 1})), mock.patch.object(provision.shutil, 'which', return_value=None),
              contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(Finished)):
            self.service('docker').runtime_ready()
        self.assertIn('dockerd is not installed', error.getvalue())

    def test_the_deps_image_is_pulled_tagged_or_built_per_runtime(self):
        for ctr in ('docker', 'wslc', 'container'):
            with self.subTest(ctr=ctr):
                self.calls.clear()
                with mock.patch.object(provision, 'run', self.runner({'inspect': 1})):
                    self.service(ctr).build_deps_image()
                inspect, pull, tag = self.commands()
                self.assertEqual(inspect, provision.runtime_command([ctr, 'image', 'inspect', 'deps:tag']))
                self.assertEqual(pull, provision.runtime_command([ctr, 'pull', '--quiet', '--platform', 'linux/amd64', 'pub:tag-deps']))
                self.assertEqual(tag, provision.runtime_command([ctr, 'tag', 'pub:tag-deps', 'deps:tag']))
        self.assertEqual(pull, ['container', 'image', 'pull', '--platform', 'linux/amd64', 'pub:tag-deps'])
        self.calls.clear()
        with mock.patch.object(provision, 'run', self.runner({'inspect': 1, 'pull': 1})), contextlib.redirect_stderr(io.StringIO()):
            service = self.service('wslc')
            (service.cache / 'pull-deps.err').write_text('')
            service.retry = lambda command: self.calls.append(['retry', *map(str, command)])
            service.build_deps_image()
        self.assertEqual(self.commands()[-2], ['retry', 'wslc', 'pull', '--quiet', 'debian:testing'])
        self.assertEqual(self.commands()[-1][:4], ['wslc', 'build', '--target', 'deps'])
        self.calls.clear()
        with mock.patch.object(provision, 'run', self.runner({'inspect': 1})):
            service = self.service('container')
            service.pull = False
            service.retry = lambda command: self.calls.append(['retry', *map(str, command)])
            service.build_deps_image()
        self.assertEqual(self.commands()[-1][:6], ['container', 'build', '--cpus', '4', '--memory', '6G'])

    def libraries(self, ctr, host='Linux', pulled=True):
        service = self.service(ctr, host)
        states = iter([False, True, True])
        service.libs_ok = lambda cell, prefix=None: next(states)
        service.build_deps_image = lambda: self.calls.append(['deps'])
        service.pool_phase = lambda *args: self.calls.append(['pool']) or 0
        service.in_deps_image = lambda script, *args: self.calls.append(['in-deps', *args])
        with (mock.patch.object(provision, 'run', self.runner({'pull': 0 if pulled else 1})),
              mock.patch.object(provision, 'text', lambda command, **kwargs: self.calls.append([str(p) for p in command]) or 'cid\r'),
              contextlib.redirect_stderr(io.StringIO())):
            (service.cache / 'pull-published.err').write_text('manifest unknown\n')
            service.libraries()
        return self.commands()

    def test_the_published_libraries_are_copied_per_runtime(self):
        commands = self.libraries('docker')
        self.assertEqual(commands[0], ['docker', 'pull', '--quiet', '--platform', 'linux/amd64', 'pub:tag'])
        self.assertEqual(commands[1], ['docker', 'create', '--platform', 'linux/amd64', 'pub:tag', '/none'])
        self.assertEqual(commands[2], ['docker', 'cp', 'cid:/opt/pinyin-oracle/tkrzw', str(self.work / 'cache/oracle/tkrzw')])
        self.assertEqual(commands[3], ['docker', 'rm', 'cid'])

    def test_on_windows_the_published_prefix_is_unpacked_by_the_deps_image(self):
        self.calls.clear()
        commands = self.libraries('wslc', 'Windows')
        self.assertEqual(commands[:2], [['wslc', 'pull', '--quiet', 'pub:tag'], ['wslc', 'create', 'pub:tag', '/none']])
        self.assertEqual(commands[2], ['wslc', 'export', '-o', str(self.work / 'cache/published.tar'), 'cid'])
        self.assertEqual(commands[3], ['in-deps', 'tkrzw'])
        self.assertEqual(commands[4], ['wslc', 'remove', 'cid'])
        self.assertFalse(any('cp' in command for command in commands))

    def test_apple_builds_the_libraries_in_the_deps_image_without_the_published_pull(self):
        self.calls.clear()
        commands = self.libraries('container', 'Darwin')
        self.assertEqual(commands, [['deps'], ['pool']])
        self.assertIn(['log', 'Apple container: building the libraries in the deps image (no published-image pull)'], self.calls)

    def test_windows_prefix_checks_and_removals_run_in_the_deps_image(self):
        service = self.service('wslc', 'Windows')
        service.contained_ready = lambda: None
        with (mock.patch.object(provision, 'run', self.runner({'libs': provision.PREFIX_INVALID})),
              mock.patch.object(provision.images, 'prefix_ok') as host_check,
              mock.patch.object(provision.shutil, 'rmtree') as rmtree):
            self.assertFalse(service.libs_ok('tkrzw'))
            self.assertTrue(service.data_ok('kc', service.oracle_root / 'kc'))
            service.remove_prefix('bdb')
        host_check.assert_not_called()
        rmtree.assert_not_called()
        libs, data, remove = self.commands()
        self.assertEqual(libs[:3], ['wslc', 'run', '--rm'])
        self.assertIn(f'{service.oracle_root}:/opt/pinyin-oracle:ro', libs)
        self.assertEqual(libs[libs.index('deps:tag'):], ['deps:tag', 'uv', 'run', '--no-project', '--managed-python', 'python',
                                                         '-X', 'lazy_imports=normal', '-m', 'tools.dev.provision',
                                                         'prefix-check', 'libs', 'tkrzw',
                                                         '/opt/pinyin-oracle/tkrzw', '/run/oracle-pin.txt'])
        self.assertEqual(data[-4:], ['data', 'kc', '/opt/pinyin-oracle/kc', '/run/oracle-pin.txt'])
        self.assertEqual(remove[-4:], ['-c', 'rm -rf "/cache/oracle/$1"', 'sh', 'bdb'])
        with mock.patch.object(provision.shutil, 'rmtree') as rmtree:
            self.service('docker').remove_prefix('bdb')
        rmtree.assert_called_once()
        self.calls.clear()
        with mock.patch.object(provision, 'run', self.runner()):
            self.assertEqual(service.contained('summary', 'tkrzw', service.oracle_root / 'tkrzw'), 0)
        self.assertEqual(self.commands()[0][-5:], ['summary', 'tkrzw', '/opt/pinyin-oracle/tkrzw', '/run/oracle-pin.txt',
                                                   str(service.oracle_root / 'tkrzw')])

    def test_a_check_that_does_not_run_keeps_the_prefix_and_names_the_error(self):
        # 2: uv or the tools.dev interpreter gate; 125: the runtime; 1: a crash in the check.
        for status in (1, 2, 125):
            with self.subTest(status=status):
                self.calls.clear()
                service = self.service('wslc', 'Windows')
                service.contained_ready = lambda: None
                prefix = service.oracle_root / 'tkrzw'
                (prefix / 'lib').mkdir(parents=True, exist_ok=True)
                (prefix / 'lib/libpinyin.so').write_text('kept')
                with (mock.patch.object(provision, 'run', self.runner({'prefix-check': status})),
                      contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(Finished)):
                    service.libraries()
                self.assertIn(f'tkrzw: the libs check of {prefix} did not run (wslc exited {status}); the prefix is kept',
                              error.getvalue())
                self.assertEqual((prefix / 'lib/libpinyin.so').read_text(), 'kept')
                commands = self.commands()
                self.assertEqual(len(commands), 1)
                self.assertIn('prefix-check', commands[0])
                self.assertFalse(any('rm -rf' in ' '.join(command) or 'export' in command for command in commands))

    def test_an_invalid_verdict_is_a_status_of_its_own(self):
        with mock.patch.object(provision.Provision, 'options', lambda self, argv: None):
            for check in ('libs', 'data'):
                with self.subTest(check=check), mock.patch.object(provision.Provision, check + '_ok', return_value=False):
                    self.assertEqual(provision.main(['prefix-check', check, 'tkrzw', '/p', '/pin']), provision.PREFIX_INVALID)
                with mock.patch.object(provision.Provision, check + '_ok', return_value=True):
                    self.assertEqual(provision.main(['prefix-check', check, 'tkrzw', '/p', '/pin']), 0)

    def test_a_tkrzw_loader_check_that_does_not_run_keeps_the_cache(self):
        service = self.service('podman')
        prefix = service.tkrzw_prefix
        (prefix / 'bin').mkdir(parents=True)
        (prefix / 'bin/tkrzw_dbm_util').write_text('built')
        (prefix / 'REV').write_text(provision.TKRZW_REV + '\n')
        uncertified = mock.Mock(status=1, stdout=b'', stderr=b'')
        with (mock.patch.object(provision, 'capture', return_value=uncertified),
              mock.patch.object(provision, 'resolve_tkrzw', side_effect=Finished(127)),
              mock.patch.object(provision, 'run') as run,
              contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(Finished)):
            service.sound_tkrzw()
        self.assertIn(f'tkrzw: the loader check of {prefix} did not run (status 127); the cache is kept', error.getvalue())
        self.assertEqual((prefix / 'bin/tkrzw_dbm_util').read_text(), 'built')
        run.assert_not_called()

    def test_valgrind_ext4_and_dump_utilities_off_linux(self):
        for host in ('Darwin', 'Windows'):
            with self.subTest(host=host):
                service = self.service('docker', host)
                service.cells = ['bdb', 'kc', 'tkrzw']
                with (mock.patch.object(service, 'which', side_effect=lambda name: name in ('cargo', 'rustup')),
                      mock.patch.object(provision, 'run', return_value=0) as run):
                    service.prerequisites()
                self.assertEqual(service.valgrind_version, 'not checked (Linux only)')
                self.assertFalse(any('valgrind' in map(str, call.args[0]) for call in run.call_args_list))
                with contextlib.redirect_stdout(io.StringIO()) as output:
                    service.report_ext4()
                self.assertEqual(output.getvalue(), 'ext4 paths: not checked (findmnt and the measurement gates are Linux only).\n')
                # Provisioning runs neither dump tool, and off Linux its checks run in the deps image.
                with (mock.patch.object(service, 'which', return_value=None), mock.patch.object(service, 'apt') as apt,
                      mock.patch.object(provision.os, 'geteuid', create=True) as geteuid):
                    service.backend_tools()
                apt.assert_not_called()
                geteuid.assert_not_called()
                self.assertIn(['log', f'db5.3_dump kchashmgr: not required on {host}; '
                                      'provisioning checks run in the deps image there'], self.calls)

    def test_linux_still_requires_the_dump_utilities(self):
        service = self.service('podman')
        service.cells = ['bdb', 'kc']
        with (mock.patch.object(service, 'which', return_value=None), mock.patch.object(service, 'apt') as apt,
              mock.patch.object(provision.os, 'geteuid', create=True, return_value=1000),
              contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(Finished)):
            service.backend_tools()
        apt.assert_not_called()
        self.assertIn('missing: db5.3_dump kchashmgr\nrun as root: apt-get install -y db5.3-util kyotocabinet-utils',
                      error.getvalue())

    def test_provision_and_provision_oracle_share_the_keyed_entries_of_one_root(self):
        env = {'PATH': '', 'OXPINYIN_CACHE': str(self.work / 'root')}
        commit = provision.images.fields(ROOT / 'tools/oracle/oracle-pin.txt')['libpinyin_commit']
        service = provision.Provision(root=ROOT, env=env)
        self.assertEqual(service.cache, self.work / 'root' / f'pin-{commit[:12]}')
        self.assertEqual(service.cache, provision.images.provision_cache(env))
        self.assertEqual(service.tkrzw_prefix, (self.work / 'root' / f'tkrzw-{provision.TKRZW_REV[:12]}').resolve())
        self.assertEqual(provision.images.model_cache(env), provision.model.default_cache(env))

    def test_the_cache_root_is_needed_only_by_a_run_that_uses_it(self):
        service = provision.Provision(root=ROOT, env={'PATH': ''})
        with contextlib.suppress(Finished):
            service.options(['--python-only'])
        self.assertTrue(service.python_only)
        with self.assertRaisesRegex(SystemExit, 'OXPINYIN_CACHE must name an absolute'):
            service.cache

    def test_cache_dir_names_the_provisioning_cache_only_inside_the_image(self):
        service = provision.Provision(root=ROOT, env={'PATH': '', 'OXPINYIN_CACHE': str(self.work / 'root')})
        with contextlib.redirect_stderr(io.StringIO()) as error, self.assertRaises(Finished) as raised:
            service.options(['--cache-dir', '/provision'])
        self.assertEqual((raised.exception.status, error.getvalue()), (2, 'unknown option: --cache-dir\n'))
        with contextlib.suppress(Finished):
            service.options(['--cache-dir', '/provision'], in_image=True)
        self.assertEqual(service.cache, Path('/provision'))
        self.assertEqual(service.tkrzw_prefix.parent, (self.work / 'root').resolve())

    def test_the_platform_comes_from_the_host_architecture_on_every_os(self):
        for machine, expected in (('x86_64', 'linux/amd64'), ('AMD64', 'linux/amd64'), ('aarch64', 'linux/arm64'),
                                  ('ARM64', 'linux/arm64')):
            with self.subTest(machine=machine), mock.patch.object(provision.platform, 'machine', return_value=machine):
                service = self.service('docker')
                service.options(['--cells', 'tkrzw'])
                self.assertEqual(service.platform, expected)
        with (mock.patch.object(provision.platform, 'machine', return_value='riscv64'),
              contextlib.redirect_stderr(io.StringIO()), self.assertRaises(Finished) as raised):
            self.service('docker').options([])
        self.assertEqual(raised.exception.status, 2)


class ManifestCheckTests(unittest.TestCase):
    """check_manifest gives sha256sum --check --quiet's status and output."""
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.work = Path(directory.name)
        for name in ('a', 'b'):
            (self.work / name).write_text(name + '\n')

    def manifest(self, *lines):
        (self.work / 'm.sha256').write_text(''.join(line + '\n' for line in lines))
        return provision.check_manifest('m.sha256', self.work)

    def test_mismatches_unreadable_files_and_malformed_lines_follow_sha256sum(self):
        a = digest_file(self.work / 'a')
        status, output = self.manifest(f'{"0" * 64}  a', f'{a}  gone', 'not a checksum line', f'{"1" * 64}  b')
        self.assertEqual(status, 1)
        lines = output.splitlines()
        self.assertEqual(lines[0], 'a: FAILED')
        self.assertRegex(lines[1], r'^sha256sum: gone: .+')
        self.assertEqual(lines[2:], ['gone: FAILED open or read', 'b: FAILED',
                                     'sha256sum: WARNING: 1 line is improperly formatted',
                                     'sha256sum: WARNING: 1 listed file could not be read',
                                     'sha256sum: WARNING: 2 computed checksums did NOT match'])

    def test_a_manifest_without_a_checksum_line_fails(self):
        self.assertEqual(self.manifest('nothing here'),
                         (1, 'sha256sum: m.sha256: no properly formatted checksum lines found\n'))
        status, output = provision.check_manifest('absent.sha256', self.work)
        self.assertEqual(status, 1)
        self.assertTrue(output.startswith('sha256sum: absent.sha256: '))


class InImageCommandTests(unittest.TestCase):
    def test_deps_image_run_takes_the_builder_from_the_mounted_checkout(self):
        # The checkout is the only bind that carries recipe inputs: the builder, its pin record
        # and the hashed files all come from it, never from a stale copy in a pulled deps image.
        run = object.__new__(provision.Provision)
        run.ctr, run.platform = 'podman', 'linux/amd64'
        run.net_flags, run.env_flags = [], []
        run.root, run.pin = Path('/checkout'), Path('/checkout/tools/oracle/oracle-pin.txt')
        run.cache, run.model_cache, run.deps_image = Path('/cache'), Path('/model'), 'deps:tag'
        command = run.in_image_command(['--libs-only'])
        self.assertIn(f'{run.root}:/opt/oracle-recipe:ro,z', command)
        self.assertEqual(command[-3:], ['python3', '/opt/oracle-recipe/tools/oracle/build.py', '--libs-only'])
        self.assertFalse(any('/usr/local/' in str(part) for part in command))
        run.ctr = 'container'
        # `container` names Apple's CLI only on macOS; on Windows it is wslc's alias.
        with mock.patch.object(provision.images, 'host_system', return_value='Darwin'):
            command = run.in_image_command(['--libs-only'])
        self.assertIn(f'type=bind,source={run.root},target=/opt/oracle-recipe,readonly', command)
        self.assertEqual(command[command.index('--memory') + 1], '6G')
        self.assertEqual(command[-3:], ['python3', '/opt/oracle-recipe/tools/oracle/build.py', '--libs-only'])
        self.assertFalse(any(f'source={run.root / "tools"}' in part for part in command))


if __name__ == '__main__':
    unittest.main()
