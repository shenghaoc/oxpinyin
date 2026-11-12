"""Windows compare contracts; synthetic DLLs, not product measurements."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import bench, identity, prepare, recipes, reports
from tools.dev.runners import Finished


class WindowsReports(unittest.TestCase):
    def test_commit_uses_paired_bytes_and_missing_metrics_fail(self):
        row = dict(pid=1, cycles=1, operations=123, fingerprint='a'*16,
                   wall_seconds=1., cpu_seconds=.1, loop_cpu_cycles=1000,
                   peak_private_commit_bytes=100, peak_working_set_bytes=200)
        rows = []
        for block, a, b in ((0, 100, 140), (1, 900, 920)):
            for arm, value in (('A', a), ('B', b)):
                rows.append(dict(row, kind='comparison', block=block, arm=arm,
                                 peak_private_commit_bytes=value,
                                 loop_cpu_cycles=1000 if arm == 'A' else 1500))
        summary = reports.comparison(rows, windows=True)
        result = summary['comparison']['peak_private_commit_bytes']
        self.assertNotIn('B_over_A', result)
        self.assertEqual(result['B_minus_A_bytes']['median'], 30)
        # Cycles and wall time lead; cycles are a count with a B/A ratio, never seconds.
        self.assertEqual(list(summary['comparison'])[:2], ['loop_cpu_cycles', 'wall_seconds'])
        self.assertEqual(summary['comparison']['loop_cpu_cycles']['B_over_A']['median'], 1.5)
        self.assertEqual(set(summary['units']), set(reports.WINDOWS_MEASURES))
        self.assertIn('QueryProcessCycleTime', summary['units']['loop_cpu_cycles'])
        self.assertIn('not convertible to seconds', summary['units']['loop_cpu_cycles'])
        self.assertIn('1/64-second', summary['units']['cpu_seconds'])
        for key in reports.WINDOWS_MEASURES:
            for bad in (None, -1, True):
                with self.subTest(key=key, bad=bad), self.assertRaises(ValueError):
                    reports.observation(json.dumps(dict(row, **{key: bad})), cycles=1,
                                        operations=123, windows=True)
        # Only the GetProcessTimes figure may read zero; a zero or fractional cycle delta is broken.
        reports.observation(json.dumps(dict(row, cpu_seconds=0.)), cycles=1, operations=123, windows=True)
        for bad in (0, 1.5):
            with self.subTest(cycles=bad), self.assertRaises(ValueError):
                reports.observation(json.dumps(dict(row, loop_cpu_cycles=bad)), cycles=1,
                                    operations=123, windows=True)

    def test_zero_cpu_sample_drops_only_that_ratio_and_the_campaign_completes(self):
        operations = recipes.workload_operations()
        row = dict(pid=1, cycles=1, operations=operations, fingerprint='a'*16,
                   wall_seconds=1., cpu_seconds=.25, loop_cpu_cycles=1000,
                   peak_private_commit_bytes=100, peak_working_set_bytes=200)
        # One zero GetProcessTimes reading, in a control sample, among eight.
        samples = [dict(row, cpu_seconds=0.) if index == 3 else dict(row) for index in range(8)]
        args = types.SimpleNamespace(pairs=2, control_pairs=2, cycles=1, budget=180, windows=True)
        arms = {'A': {}, 'B': {}, recipes.CONTROL: {}}
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(recipes, 'native_sample', side_effect=samples) as sample, \
             contextlib.redirect_stdout(io.StringIO()):
            recipes.native_compare(args, arms, directory, {}, Path(directory))
            self.assertEqual(sample.call_count, 8)
            self.assertEqual(len((Path(directory)/'runs.jsonl').read_text().splitlines()), 8)
            summary = json.loads((Path(directory)/'summary.json').read_text())
        for kind in ('comparison', 'control'):
            cpu = summary[kind]['cpu_seconds']
            self.assertNotIn('B_over_A', cpu)
            self.assertIn('below the 1/64-second', cpu['no_ratio'])
            self.assertEqual(cpu['A']['n'], 2)
            for metric in ('loop_cpu_cycles', 'wall_seconds'):
                self.assertEqual(summary[kind][metric]['B_over_A']['median'], 1)
                self.assertNotIn('no_ratio', summary[kind][metric])
            self.assertEqual(summary[kind]['peak_private_commit_bytes']['B_minus_A_bytes']['median'], 0)
        # A zero cycle delta is still a broken capture and stops the campaign at that sample.
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(recipes, 'native_sample', return_value=dict(row, loop_cpu_cycles=0)) as sample:
            with self.assertRaisesRegex(ValueError, 'broken required capture: loop_cpu_cycles'):
                recipes.native_compare(args, arms, directory, {}, Path(directory))
            sample.assert_called_once()

    def test_input_commit_rounds_each_mapped_file_not_dbms_or_addons(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root/'table.conf').write_text('default GB_DICTIONARY x a.bin u SYSTEM_FILE\n'
                'default GBK_DICTIONARY x b.bin u SYSTEM_FILE\n'
                'addon 4 x huge.bin u SYSTEM_FILE\n')
            (root/'a.bin').write_bytes(b'x'*4097)
            (root/'b.bin').write_bytes(b'x'*2)
            result = identity.expected_cow_commit(root, page_size=4096)
            self.assertEqual(result['bytes'], 12288)
            self.assertEqual(len(result['files']), 2)

    def test_module_identity_rejects_other_files_and_changed_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory)/'a.dll', Path(directory)/'b.dll'
            a.write_bytes(b'a'); b.write_bytes(b'a')
            expected = identity.module_inventory([str(a)])
            with self.assertRaises(ValueError):
                identity.verify_modules([str(b)], expected)
            a.write_bytes(b'b')
            with self.assertRaises(ValueError):
                identity.verify_modules([str(a)], expected)


if sys.platform == 'win32':
    class WindowsDriver(unittest.TestCase):
        @classmethod
        def setUpClass(cls):
            from tools.dev.check import msvc_environment
            cls.scratch = tempfile.TemporaryDirectory()
            cls.root = Path(cls.scratch.name)
            cls.env = {k.upper(): v for k, v in msvc_environment(dict(os.environ)).items()}
            installed = Path(os.environ['VCPKG_ROOT'])/'installed/x64-windows'
            cls.env['PATH'] = str(installed/'bin') + os.pathsep + cls.env['PATH']
            cls.env['TEMP'] = cls.env['TMP'] = str(cls.root)
            cl = shutil.which('cl', path=cls.env['PATH'])
            cls.lib = cls.root/'pinyin_capi.dll'
            source = cls.root/'stub.c'
            source.write_text('''#include <windows.h>
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
__declspec(dllimport) void g_free(void *);
static bool failed, late, system_late;
__declspec(dllexport) void *pinyin_init(const char *data,const char *user) { (void)user;failed=!strcmp(data,"fail");late=!strcmp(data,"late");system_late=!strcmp(data,"system");return (void*)1; }
__declspec(dllexport) void pinyin_fini(void *p) { (void)p;g_free(NULL);if(late) LoadLibraryW(L"late.dll");if(system_late) LoadLibraryW(L"version.dll"); }
__declspec(dllexport) void *pinyin_alloc_instance(void *p) { return p; }
__declspec(dllexport) void pinyin_free_instance(void *p) { (void)p; }
__declspec(dllexport) bool pinyin_set_options(void *p,unsigned f) { (void)p;(void)f;return true; }
__declspec(dllexport) size_t pinyin_parse_more_full_pinyins(void *p,const char *s) { (void)p;return strlen(s); }
__declspec(dllexport) bool pinyin_guess_candidates(void *p,size_t n,unsigned f) { (void)p;(void)n;(void)f;return !failed; }
__declspec(dllexport) bool pinyin_get_n_candidate(void *p,unsigned *n) { (void)p;*n=1;return true; }
__declspec(dllexport) bool pinyin_reset(void *p) { (void)p;return true; }
''')
            done = subprocess.run([cl,'/nologo','/LD','/MD','/std:c11',source,
                '/Fe:'+str(cls.lib),'/Fo:'+str(cls.root/'stub.obj'),str(installed/'lib/glib-2.0.lib'),
                '/link','/IMPLIB:'+str(cls.root/'pinyin_capi.dll.lib')], env=cls.env,capture_output=True)
            if done.returncode:
                raise AssertionError(done.stdout.decode(errors='replace')+done.stderr.decode(errors='replace'))
            # A late product dependency: beside the driver, outside the system directory.
            (cls.root/'late.c').write_text('__declspec(dllexport) int late_marker(void) { return 1; }')
            done = subprocess.run([cl,'/nologo','/LD','/MD',cls.root/'late.c','/Fe:'+str(cls.root/'late.dll'),
                '/Fo:'+str(cls.root/'late.obj')], env=cls.env,capture_output=True)
            if done.returncode:
                raise AssertionError(done.stdout.decode(errors='replace')+done.stderr.decode(errors='replace'))
            cls.driver = cls.root/'bisect.exe'
            prepare.native_driver(dict(windows=True, import_library=cls.root/'pinyin_capi.dll.lib'),cls.driver,cls.env)
            cls.env['MEASURE_PREFLIGHT'] = '1'
            done = cls.invoke()
            if done.returncode:
                raise AssertionError(done.stderr.decode())
            cls.modules = json.loads(done.stdout)['modules']
            del cls.env['MEASURE_PREFLIGHT']
            cls.manifest = cls.root/'modules.txt'
            cls.manifest.write_text(''.join(path+'\n' for path in cls.modules),encoding='utf-8')
            cls.env['MEASURE_MODULES'] = str(cls.manifest)

        @classmethod
        def tearDownClass(cls):
            cls.scratch.cleanup()

        @classmethod
        def invoke(cls, path=None, data='good', env=None):
            return subprocess.run([cls.driver,'--measure',path or cls.lib,'unused',data],
                env={**cls.env,'PERF_CYCLES':'1000',**(env or {})},capture_output=True,timeout=15)

        def test_wrong_dll_and_failed_workload(self):
            other = self.root/'other.dll'
            shutil.copy2(self.lib, other)
            for changes, message in (({'path':other}, b'wrong loaded image'),
                                      ({'data':'fail'}, b'incomplete workload')):
                with self.subTest(changes=changes):
                    done=self.invoke(**changes)
                    self.assertNotEqual(done.returncode,0)
                    self.assertEqual(done.stdout,b'')
                    self.assertIn(message,done.stderr)

        def test_capture_and_dependency_manifest(self):
            done=self.invoke()
            self.assertEqual(done.returncode,0,done.stderr)
            row=reports.observation(done.stdout,cycles=1000,operations=123000,windows=True)
            identity.verify_modules(row['modules'],identity.module_inventory(self.modules))
            bad=self.root/'bad-modules.txt';bad.write_text('not-the-runtime.dll\n')
            done=self.invoke(env={'MEASURE_MODULES':str(bad)})
            self.assertNotEqual(done.returncode,0)
            self.assertIn(b'identity mismatch before loop',done.stderr)
            self.assertEqual(done.stdout,b'')

        def test_completed_module_list_rejects_a_late_dependency(self):
            self.assertFalse(any(Path(path).name.lower() == 'late.dll' for path in self.modules))
            done=self.invoke(data='late')
            self.assertNotEqual(done.returncode,0)
            self.assertEqual(done.stdout,b'')
            self.assertIn(b'capture/dependency identity',done.stderr)

        def test_identity_covers_the_dependency_set_not_host_system_modules(self):
            system = Path(os.environ['SystemRoot'])/'System32'
            names = {Path(path).name.lower() for path in self.modules}
            self.assertLessEqual({'glib-2.0-0.dll', 'ucrtbase.dll', 'vcruntime140.dll'}, names)
            for path in self.modules:
                if Path(path).parent.samefile(system):
                    self.assertRegex(Path(path).name.lower(), '^(ucrtbase|vcruntime|msvcp1)')
            # The loader maps apphelp.dll on a new path's first launch only; neither that nor
            # a system module loaded late is a product dependency.
            fresh = self.root/'fresh.exe'
            shutil.copy2(self.driver, fresh)
            for _ in range(2):
                done = subprocess.run([fresh,'--measure',self.lib,'unused','system'],
                    env={**self.env,'PERF_CYCLES':'1000'},capture_output=True,timeout=15)
                self.assertEqual(done.returncode,0,done.stderr)
                identity.verify_modules(json.loads(done.stdout)['modules'],identity.module_inventory(self.modules))

        def test_glib_ownership_required(self):
            source = self.root/'stub.c'
            no_glib = self.root/'no-glib.c'
            no_glib.write_text(source.read_text().replace('g_free(NULL);', ''))
            bad = self.root/'no-glib.dll'
            cl = shutil.which('cl', path=self.env['PATH'])
            done = subprocess.run([cl,'/nologo','/LD','/MD','/std:c11',no_glib,
                '/Fe:'+str(bad),'/Fo:'+str(self.root/'no-glib.obj')],
                env=self.env,capture_output=True)
            self.assertEqual(done.returncode,0,done.stdout)
            payload=self.lib.read_bytes()
            try:
                self.lib.write_bytes(bad.read_bytes())
                done=self.invoke()
                self.assertNotEqual(done.returncode,0)
                self.assertIn(b'GLib ownership',done.stderr)
            finally:
                self.lib.write_bytes(payload)

    class WindowsPreparation(unittest.TestCase):
        def test_release_build_has_no_overrides(self):
            metadata={'packages':[{'name':'oxpinyin-capi','features':{'default':['tkrzw']}}]}
            with tempfile.TemporaryDirectory() as directory:
                root=Path(directory);pc=root/'installed/x64-windows/lib/pkgconfig'
                pc.mkdir(parents=True);(pc/'tkrzw.pc').touch()
                with mock.patch('tools.dev.check.msvc_environment',side_effect=lambda env:env), \
                     mock.patch.object(bench,'run') as run, \
                     mock.patch.object(bench,'capture',return_value=types.SimpleNamespace(status=0,stdout=json.dumps(metadata).encode())), \
                     mock.patch.object(identity,'sha256',return_value='digest'):
                    arm=prepare.native_build('a'*40,root/'build',dict(VCPKG_ROOT=directory,
                        RUSTFLAGS='bad',CARGO_ENCODED_RUSTFLAGS='bad',CARGO_PROFILE_RELEASE_DEBUG='2'),windows=True)
                command=next(call for call in run.call_args_list if call.args[0][0]=='cargo')
                self.assertEqual(command.args[0],['cargo','build','--locked','--release','-p','oxpinyin-capi'])
                self.assertFalse(any(key in command.kwargs['env'] for key in
                                    ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_PROFILE_RELEASE_DEBUG')))
                self.assertEqual(arm['dylib'].name,'pinyin_capi.dll')
                self.assertEqual(command.kwargs['env']['PKG_CONFIG_PATH'],str(pc.resolve()))

        def test_one_build_identical_drivers_and_cleanup_on_success_and_failure(self):
            from tools.dev import contract_data
            for outcome in (None, ValueError('sample failed')):
                with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as directory:
                    root=Path(directory);destinations=[];digests=[];staged={}
                    def build(sha, destination, env, **options):
                        destinations.append(destination)
                        destination.mkdir(parents=True)
                        dll=destination/'pinyin_capi.dll';dll.write_bytes(b'product')
                        return dict(revision=sha,dylib=dll,uuid='hash',windows=True)
                    def driver(arm, output, env):
                        output.write_bytes(b'identical driver')
                        return identity.sha256(output)
                    def compare(args, arms, data, env, output):
                        digests.extend(identity.sha256(arm['driver']) for arm in arms.values())
                        staged.update({label: (arm['dylib'], arm['dylib'].read_bytes(), arm['driver'].parent)
                                       for label, arm in arms.items()})
                        if outcome: raise outcome
                    with mock.patch('tools.dev.check.msvc_environment',side_effect=lambda env:env), \
                         mock.patch.object(bench,'capture',return_value=types.SimpleNamespace(status=0,stdout=b'a'*40+b'\n')), \
                         mock.patch.object(prepare,'native_build',side_effect=build) as builds, \
                         mock.patch.object(prepare,'native_driver',side_effect=driver), \
                         mock.patch.object(identity,'windows_preflight',return_value=[]), \
                         mock.patch.object(identity,'expected_cow_commit',return_value={}), \
                         mock.patch.object(contract_data,'data_identity',return_value='data'), \
                         mock.patch.object(recipes,'native_compare',side_effect=compare), \
                         contextlib.redirect_stdout(io.StringIO()):
                        args=['compare','--base','HEAD','--head','HEAD','--data',directory]
                        env=dict(OXPINYIN_CACHE=directory,VCPKG_ROOT=directory,PATH=os.environ['PATH'])
                        if outcome:
                            with self.assertRaises(ValueError): recipes.native(args,env)
                        else:
                            self.assertEqual(recipes.native(args,env),0)
                    builds.assert_called_once()
                    self.assertEqual(len(set(digests)),1)
                    # A' is a second copy of A: its own prefix, the DLL beside its driver, the same bytes.
                    self.assertEqual(set(staged), {'A', 'B', recipes.CONTROL})
                    self.assertEqual(len({path for path, _, _ in staged.values()}), 3)
                    self.assertEqual(staged[recipes.CONTROL][1], staged['A'][1])
                    self.assertEqual(staged[recipes.CONTROL][0].parent, staged[recipes.CONTROL][2])
                    self.assertEqual(staged[recipes.CONTROL][0].parents[1].name, 'C')
                    # One length for every staged prefix: the same bytes sit at same-length paths.
                    self.assertEqual({len(str(path)) for path, _, _ in staged.values()}, {len(str(staged['A'][0]))})
                    self.assertFalse(destinations[0].parent.exists())

        def test_unsupported_modes_fail_before_preparation(self):
            for argv in (['profile','--template','Allocations'],['neutrality']):
                with mock.patch.object(prepare,'native_build') as build, self.assertRaises(Finished):
                    recipes.native(argv,{})
                build.assert_not_called()


if __name__ == '__main__':
    unittest.main()
