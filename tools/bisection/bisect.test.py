"""The C harness, tools/bisection/bisect.c: its perf, sentence and measurement modes.

Each class compiles the harness and a stub library; no model or product code is loaded. The
measurement drivers also exercise the recipes, reports and neutrality checks that read their
output, because those tests share the class's compiled fixture.
"""
import contextlib
import io
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev.measure import bench, identity, neutrality, prepare, recipes, reports, sentence

SENTENCE_STUB = r'''
#include <dirent.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static char parsed[256];
static int state;
__attribute__((constructor)) static void report_user_dirs(void) {
    const char *root = getenv("REPORT_USER_ROOT");
    if (!root) return;
    DIR *dir = opendir(root);
    if (!dir) abort();
    struct dirent *entry;
    while ((entry = readdir(dir)))
        if (strncmp(entry->d_name, "bisect-", 7) == 0)
            fprintf(stderr, "DIR:%s/%s\n", root, entry->d_name);
    closedir(dir);
}
void *pinyin_init(const char *data, const char *user) {
    (void)data;
    struct stat st;
    if (stat(user, &st) || !S_ISDIR(st.st_mode)) abort();
    fprintf(stderr, "USER:%s\n", user);
    return getenv("FAIL_INIT") ? NULL : (void *)1;
}
void pinyin_fini(void *p) { (void)p; fputs("FINI\n", stderr); }
void *pinyin_alloc_instance(void *p) { fputs("ALLOC\n", stderr); return p; }
void pinyin_free_instance(void *p) { (void)p; fputs("FREE\n", stderr); }
bool pinyin_set_options(void *p, uint32_t flags) { return p && flags == 0x18a; }
bool pinyin_reset(void *p) { state=0; fputs("RESET\n", stderr); return p; }
size_t pinyin_parse_more_full_pinyins(void *p, const char *s) {
    (void)p; strcpy(parsed, s); state=1; fprintf(stderr, "PARSE:%s\n", s); return strlen(s);
}
bool pinyin_guess_sentence(void *p) {
    (void)p; if (state != 1) abort(); state=2; fputs("SENTENCE\n", stderr);
    return getenv("FAIL_SENTENCE") == NULL;
}
bool pinyin_get_sentence(void *p, uint8_t n, char **s) {
    (void)p; if (state < 2 || n) abort(); *s=strdup(parsed); return true;
}
bool pinyin_guess_candidates(void *p, size_t offset, uint32_t sort) {
    (void)p; if (state != 2 || offset || sort != 0x1e) abort();
    state=3; fputs("CANDIDATES\n", stderr); return true;
}
bool pinyin_get_n_candidate(void *p, unsigned *n) { (void)p; *n=12; return state==3; }
bool pinyin_get_candidate(void *p, unsigned n, void **c) {
    (void)p; *c=(void *)(uintptr_t)(n+1); return n < 12;
}
bool pinyin_get_candidate_type(void *p, void *c, int *t) { (void)p; *t=(uintptr_t)c==1 ? 1 : 2; return true; }
bool pinyin_get_candidate_string(void *p, void *c, const char **s) {
    (void)p; *s=(uintptr_t)c==12 ? "tail\"\\\n" : "duplicate"; return true;
}
bool pinyin_get_candidate_nbest_index(void *p, void *c, uint8_t *n) {
    (void)p; if ((uintptr_t)c != 1) abort(); *n=7; return true;
}
'''


if os.name == 'posix':
    class NativeTests(unittest.TestCase):
        @classmethod
        def setUpClass(cls):
            cls.temp = tempfile.TemporaryDirectory(prefix='sentence-harness-')
            cls.root = Path(cls.temp.name)
            cls.harness, cls.library = cls.root / 'bisect', cls.root / 'stub.so'
            source = cls.root / 'stub.c'
            source.write_text(SENTENCE_STUB)
            subprocess.run(bench.harness_build(cls.harness), check=True, capture_output=True)
            subprocess.run(['cc', '-shared', '-fPIC', '-O2', '-o', cls.library, source], check=True, capture_output=True)

        @classmethod
        def tearDownClass(cls):
            cls.temp.cleanup()

        def launch(self, mode, input='nihao', iterations='1', extra=None):
            return subprocess.run([self.harness, '--perf-' + mode, self.library, self.root, input, iterations],
                                  capture_output=True, env=dict(os.environ, **(extra or {})))

        def test_sentence_and_typing_user_directories_follow_tmpdir_and_are_removed(self):
            for mode in ('sentence', 'typing-sequence'):
                with self.subTest(mode=mode), tempfile.TemporaryDirectory(dir=self.root) as root:
                    done = self.launch(mode, extra={'TMPDIR': root})
                    self.assertEqual(done.returncode, 0, done.stderr)
                    user = Path(done.stderr.decode().splitlines()[0].removeprefix('USER:'))
                    self.assertEqual(user.parent, Path(root))
                    self.assertTrue(user.name.startswith('bisect-sentence-'))
                    self.assertFalse(user.exists())
                    self.assertEqual(list(Path(root).iterdir()), [])

        def test_sentence_and_typing_refuse_overlong_tmpdir_before_loading(self):
            for mode in ('sentence', 'typing-sequence'):
                with self.subTest(mode=mode):
                    done = self.launch(mode, extra={'TMPDIR': '/' + 'x' * 4096})
                    self.assertEqual(done.returncode, 1)
                    self.assertEqual(done.stdout, b'')
                    self.assertEqual(done.stderr, b'fatal: TMPDIR path is too long\n')

        def test_bisect_perf_places_and_cleans_user_directories_on_init_failure(self):
            for mode, prefix in ((['--perf'], 'bisect-perf-'),):
                for setting in ('custom', 'unset', 'empty'):
                    with self.subTest(mode=mode, setting=setting), \
                            tempfile.TemporaryDirectory(dir=self.root) as root:
                        parent = Path(root) if setting == 'custom' else Path('/tmp')
                        env = dict(os.environ, REPORT_USER_ROOT=str(parent), FAIL_INIT='1')
                        env.pop('TMPDIR', None)
                        if setting != 'unset':
                            env['TMPDIR'] = root if setting == 'custom' else ''
                        done = subprocess.run([self.harness, *mode, self.library, self.root],
                                              capture_output=True, env=env)
                        self.assertEqual(done.returncode, 1)
                        users = [line.removeprefix('USER:') for line in done.stderr.decode().splitlines()
                                 if line.startswith('USER:')]
                        self.assertEqual(len(users), 1, done.stderr)
                        user = Path(users[0])
                        self.assertIn(b'fatal: pinyin_init returned NULL\n', done.stderr)
                        self.assertNotIn(b'symbols missing', done.stderr)
                        self.assertEqual(user.parent, parent)
                        self.assertTrue(user.name.startswith(prefix))
                        # The loaded stub saw this directory before the failure cleanup.
                        self.assertIn('DIR:' + str(user), done.stderr.decode().splitlines())
                        self.assertFalse(user.exists())
                        self.assertEqual(list(Path(root).iterdir()), [])

        def test_sentence_calls_parse_then_sentence_without_candidates(self):
            done = self.launch('sentence', iterations='2')
            self.assertEqual(done.returncode, 0, done.stderr)
            row = sentence.observation(done.stdout, types.SimpleNamespace(workload='sentence', iterations=2, input='nihao'))
            self.assertEqual(row['observations'][0], row['observations'][1])
            self.assertEqual(done.stderr.decode().splitlines()[1:],
                             ['ALLOC', 'RESET', 'PARSE:nihao', 'SENTENCE', 'RESET', 'PARSE:nihao', 'SENTENCE', 'FREE', 'FINI'])
            user = done.stderr.decode().splitlines()[0].removeprefix('USER:')
            self.assertFalse(Path(user).exists())

        def test_typing_reuses_instance_and_retains_every_candidate_in_order(self):
            done = self.launch('typing-sequence', 'ni')
            self.assertEqual(done.returncode, 0, done.stderr)
            row = sentence.observation(done.stdout, types.SimpleNamespace(workload='typing-sequence', iterations=1, input='ni'))
            steps = row['observations'][0]
            self.assertEqual([s['sentence'] for s in steps], ['n', 'ni'])
            self.assertEqual(len(steps[0]['candidates']), 12)
            self.assertEqual(steps[0]['candidates'][0], dict(type=1, nbest=7, text='duplicate'))
            self.assertEqual(steps[0]['candidates'][-1], dict(type=2, nbest=None, text='tail"\\\n'))
            self.assertEqual(done.stderr.decode().splitlines()[1:],
                             ['ALLOC', 'RESET', 'PARSE:n', 'SENTENCE', 'CANDIDATES',
                              'PARSE:ni', 'SENTENCE', 'CANDIDATES', 'FREE', 'FINI'])

        def test_failure_is_nonzero_and_releases_instance_context_and_user_directory(self):
            done = self.launch('sentence', extra={'FAIL_SENTENCE': '1'})
            self.assertEqual(done.returncode, 1)
            lines = done.stderr.decode().splitlines()
            self.assertEqual(lines[-3:], ['FREE', 'FINI', 'sentence workload failed'])
            self.assertFalse(Path(lines[0].removeprefix('USER:')).exists())

        def test_bad_inputs_and_counts_are_rejected_before_dlopen(self):
            for text, count in [('', '1'), ('你好', '1'), ('a' * 256, '1'), ('ni', '0'),
                                ('ni', '-1'), ('ni', '100001'), ('ni', '1x'), ('ni', '')]:
                with self.subTest(text=text, count=count):
                    done = self.launch('sentence', text, count)
                    self.assertEqual(done.returncode, 2)
                    self.assertEqual(done.stdout, b'')
                    self.assertNotIn(b'USER:', done.stderr)

        def test_distinct_anchors_survive_optimization(self):
            symbols = subprocess.check_output(['nm', self.harness], text=True).splitlines()
            # Mach-O prefixes C symbols with an underscore; ELF does not.
            addresses = {s.split()[2].removeprefix('_'): s.split()[0] for s in symbols if len(s.split()) == 3}
            names = list(sentence.ANCHORS.values()) + ['bisect_perf_cold_unit', 'bisect_perf_steady_unit']
            self.assertEqual(len({addresses[name] for name in names}), 4)


if sys.platform == 'linux':
    LINUX_STUB = '''#include <stdbool.h>
#include <stddef.h>
#include <string.h>
static bool failed;
static volatile unsigned long spin;
void *pinyin_init(const char *data, const char *user) { (void)user; failed = !strcmp(data, "fail"); return failed ? NULL : (void *)1; }
void pinyin_fini(void *p) { (void)p; }
void *pinyin_alloc_instance(void *p) { return p; }
void pinyin_free_instance(void *p) { (void)p; }
bool pinyin_set_options(void *p, unsigned flags) { (void)p; (void)flags; return true; }
size_t pinyin_parse_more_full_pinyins(void *p, const char *s) { (void)p; for (int i = 0; i < 4000; i++) spin += i; return strlen(s); }
bool pinyin_guess_candidates(void *p, size_t n, unsigned f) { (void)p; (void)n; (void)f; return true; }
bool pinyin_get_n_candidate(void *p, unsigned *n) { (void)p; *n = 1; return true; }
bool pinyin_reset(void *p) { (void)p; return true; }
'''

    class LinuxDriver(unittest.TestCase):
        """The real driver, taskset and Valgrind against a stub library: no tkrzw, no product code."""

        @classmethod
        def setUpClass(cls):
            cls.scratch = tempfile.TemporaryDirectory()
            cls.root = Path(cls.scratch.name).resolve()
            (cls.root / 'stub.c').write_text(LINUX_STUB)
            cls.arms = {}
            driver = cls.root / 'bisect'
            digest = prepare.native_driver(dict(linux=True), driver, dict(os.environ))
            built = cls.root / 'build/libpinyin.so.15.0.0'
            built.parent.mkdir()
            subprocess.run(['cc', '-shared', '-fPIC', '-O1', '-g', cls.root / 'stub.c', '-o', built],
                           check=True, capture_output=True)
            for label, name in recipes.PREFIXES.items():
                arm = recipes.stage_arm(cls.root / name, dict(dylib=built, linux=True, backend='tkrzw'), driver, digest)
                cls.arms[label] = dict(arm, label=name, data=cls.root)
            cls.env = dict(os.environ, TMPDIR=str(cls.root))

        @classmethod
        def tearDownClass(cls):
            cls.scratch.cleanup()

        def test_perf_row_carries_pid_cpu_time_and_one_counter_outcome(self):
            arm = self.arms['A']
            done = subprocess.run([arm['driver'], '--perf', arm['dylib'], self.root],
                                  env=dict(self.env, PERF_MODE='rss-diag', PERF_CYCLES='6', PERF_HW='instructions',
                                           PERF_WORKLOAD_CHECKSUM='1'), capture_output=True, timeout=60)
            self.assertEqual(done.returncode, 0, done.stderr)
            row = json.loads(done.stdout)
            self.assertGreater(row['pid'], 1)
            self.assertEqual((len(row['cycles_ns']), len(row['cycles_cpu_ns'])), (6, 6))
            self.assertGreater(sum(row['cycles_cpu_ns'][1:]), 0)
            # The host grants the hardware counter or the driver reports its refusal; never both, never neither.
            self.assertEqual(('steady_hw_instructions' in row) + ('hw_errno' in row), 1)
            source = 'perf_event_open' if 'steady_hw_instructions' in row else 'callgrind'
            reduced = reports.linux_observation(done.stdout, cycles=6, library=arm['dylib'], source=source)
            self.assertGreater(reduced['private_dirty_kib'], 0)
            # A failed workload leaves no row.
            failed = subprocess.run([arm['driver'], '--perf', arm['dylib'], 'fail'], env=self.env,
                                    capture_output=True, timeout=60)
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(failed.stdout, b'')

        def test_comparison_and_profiles_run_end_to_end_on_the_stub(self):
            # Never skipped: a Linux host without these tools has not run the comparison.
            missing = [tool for tool in ('valgrind', 'taskset') if not shutil.which(tool)]
            self.assertFalse(missing, 'the end-to-end comparison needs ' + ' and '.join(missing)
                             + ' on PATH; install Valgrind and util-linux')
            args = types.SimpleNamespace(pairs=2, control_pairs=2, cycles=4, budget=600)
            output = self.root / 'session'
            output.mkdir()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                status = recipes.linux_compare(args, self.arms, self.env, output)
            summary = json.loads((output / 'summary.json').read_text())
            source = summary['instruction_source']
            runs = [json.loads(line) for line in (output / 'runs.jsonl').read_text().splitlines()]
            self.assertEqual(len(runs), 16 if source == 'callgrind' else 8)
            self.assertEqual(len({run['fingerprint'] for run in runs}), 1)
            self.assertEqual({run['instruction_source'] for run in runs}, {source})
            if source == 'callgrind':
                # The stub has no hash containers: identical bytes at same-length paths count identically.
                gate = summary['control']['loop_callgrind_ir']['gate']
                self.assertEqual((status, gate['verdict'], gate['spread']), (0, 'VALID', 0), gate)
                self.assertGreater(gate['counts']['A'][0], 3 * 123 * 4000)
            for template, name in (('Callgrind', 'callgrind.out'), ('Massif', 'massif.out')):
                out = io.StringIO()
                profile = self.root / template
                profile.mkdir()
                with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(recipes.linux_profile(types.SimpleNamespace(template=template, cycles=3, budget=600),
                                                           self.arms['A'], self.env, profile), 0)
                self.assertTrue((profile / name).is_file())
                capture = json.loads(out.getvalue())['capture']
                if template == 'Callgrind':
                    self.assertGreater(capture['subject_Ir'], 0)
                    self.assertTrue((profile / 'callgrind.out.inclusive.txt').is_file())
                else:
                    self.assertRegex(capture['heap'][0], r'^A peak heap: [1-9][0-9]* bytes$')


if sys.platform == 'darwin' and platform.machine() == 'arm64':
    class DarwinDriver(unittest.TestCase):
        @classmethod
        def setUpClass(cls):
            cls.scratch = tempfile.TemporaryDirectory()
            cls.root = Path(cls.scratch.name)
            cls.lib = cls.root/'libpinyin.15.0.0.dylib'
            source = cls.root/'stub.c'
            source.write_text('''#include <stdbool.h>
#include <stddef.h>
#include <string.h>
static bool failed;
void *pinyin_init(const char *data, const char *user) { (void)user; failed=!strcmp(data,"fail"); return (void*)1; }
void pinyin_fini(void *p) { (void)p; }
void *pinyin_alloc_instance(void *p) { return p; }
void pinyin_free_instance(void *p) { (void)p; }
bool pinyin_set_options(void *p, unsigned flags) { (void)p;(void)flags;return true; }
size_t pinyin_parse_more_full_pinyins(void *p,const char *s) { (void)p;return strlen(s); }
bool pinyin_guess_candidates(void *p,size_t n,unsigned f) { (void)p;(void)n;(void)f;return !failed; }
bool pinyin_get_n_candidate(void *p,unsigned *n) { (void)p;*n=1;return true; }
bool pinyin_reset(void *p) { (void)p;return true; }
''')
            subprocess.run(['cc','-dynamiclib','-g',str(source),'-o',str(cls.lib)],check=True,capture_output=True)
            cls.uuid = identity.macho_uuid(cls.lib)
            cls.driver = cls.root/'bin/bisect'
            prepare.native_driver({'dylib':cls.lib}, cls.driver, os.environ)
            (cls.root/'lib').mkdir()
            import shutil
            shutil.copy2(cls.lib,cls.root/'lib'/cls.lib.name)
            cls.loaded = cls.root/'lib'/cls.lib.name

        @classmethod
        def tearDownClass(cls):
            cls.scratch.cleanup()

        def invoke(self, path=None, uuid=None, data='good'):
            return subprocess.run([str(self.driver),'--measure',str(path or self.loaded),uuid or self.uuid,data],
                                  env={**os.environ,'PERF_CYCLES':'1000'},capture_output=True,timeout=10)

        def test_wrong_image_and_uuid_fail_before_result(self):
            for path, uuid in ((self.lib,self.uuid),(self.loaded,'f'*32)):
                result=self.invoke(path,uuid)
                self.assertNotEqual(result.returncode,0)
                self.assertEqual(result.stdout,b'')
                self.assertIn(b'wrong loaded image',result.stderr)

        def test_failed_workload_has_no_result(self):
            result=self.invoke(data='fail')
            self.assertNotEqual(result.returncode,0)
            self.assertEqual(result.stdout,b'')
            self.assertIn(b'incomplete workload',result.stderr)

        def test_native_capture_units_count_and_signed_driver(self):
            raw,usage=bench.measured_process([self.driver,'--measure',self.loaded,self.uuid,'good'],
                env={**os.environ,'PERF_CYCLES':'1000'},timeout=10)
            row=json.loads(raw)
            row['peak_rss_bytes']=usage.ru_maxrss
            reports.observation(json.dumps(row),cycles=1000,operations=1000*recipes.workload_operations())
            self.assertGreater(row['lifetime_max_phys_footprint_bytes'],1024)
            self.assertGreater(row['peak_rss_bytes'],1024)
            longer = json.loads(bench.measured_process([self.driver, '--measure', self.loaded, self.uuid, 'good'],
                env={**os.environ, 'PERF_CYCLES': '8000'}, timeout=10)[0])
            for counter in reports.COUNTS:
                self.assertGreater(longer[counter], 2 * row[counter], counter + ' must grow with the cycle count')
            subprocess.run(['codesign','--verify','--strict',self.driver],check=True,capture_output=True)
            import plistlib
            import re
            signature = subprocess.run(['codesign', '-d', '--entitlements', '-', '--xml', '--verbose=4', self.driver],
                                       check=True, capture_output=True)
            self.assertIs(plistlib.loads(signature.stdout)['com.apple.security.get-task-allow'], True)
            flags = int(re.search(rb'flags=0x([0-9a-fA-F]+)', signature.stderr)[1], 16)
            self.assertEqual(flags & 0x10000, 0, 'driver must not use hardened runtime')
            self.assertEqual(self.lib.read_bytes(), self.loaded.read_bytes(), 'product dylib must not be re-signed')


        def test_macho_neutrality_rejects_code_or_address_changes(self):
            copy=self.root/'copy.dylib'
            copy.write_bytes(self.lib.read_bytes())
            self.assertIn('identical',neutrality.compare_macho(self.lib,copy))
            original=neutrality.macho_code(copy)
            changed={key:(address+4,data) for key,(address,data) in original.items()}
            with mock.patch.object(neutrality,'macho_code',side_effect=[original,changed]):
                with self.assertRaisesRegex(ValueError,'sections or addresses'):
                    neutrality.compare_macho(self.lib,copy)
            with mock.patch.object(bench,'capture',return_value=types.SimpleNamespace(status=0,stdout=b'')):
                with self.assertRaisesRegex(ValueError,'empty arm64'):
                    neutrality.compare_macho(self.lib,copy)


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


if __name__ == '__main__':
    unittest.main()
