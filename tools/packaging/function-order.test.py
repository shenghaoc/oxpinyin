#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Source regressions for the optional shipping link ordering; no compiler or
profiler is needed.

Synthetic nm records exercise capability probing, policy resolution, fallback
transport and cleanup. Real-library placement, reproducible builds, fallback
byte identity and performance still need the shipping gate.
"""

from contextlib import contextmanager
import argparse
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import runpy
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock


HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import pool, test_support

LIBRARY = Path("/synthetic/libpinyin.so")
HEADER = "oxpinyin-function-order-v1\tlibpinyin.so.15\n"
ORDER = runpy.run_path(str(HERE / 'function_order.py'))


def row(name, section, address="0000000000000000"):
    return f"{name}|{address}| t | FUNC |0000000000000010| |{section}\n"


@contextmanager
def scratch():
    # The executable tests create only task-private operational scratch.
    # Preserve the caller's handlers and clean even on INT/TERM.
    with tempfile.TemporaryDirectory(prefix="function-order-test-") as directory:
        handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}

        def interrupted(number, _frame):
            raise SystemExit(128 + number)

        try:
            for sig in handlers:
                signal.signal(sig, interrupted)
            yield Path(directory)
        finally:
            for sig, handler in handlers.items():
                signal.signal(sig, handler)


class ResolverTests(unittest.TestCase):
    def invoke(self, mode, nm_text, policy=HEADER + "init\tcrate::open\n"):
        with scratch() as work:
            (work / "nm").write_text("#!/bin/sh\ncat \"$TEST_NM_INPUT\"\n")
            (work / "nm").chmod(0o755)
            (work / "symbols").write_text(nm_text)
            (work / "policy").write_text(policy)
            command = [sys.executable, "-X", "lazy_imports=normal", str(ROOT / "tools/dev/__main__.py"), "packaging", "function-order", mode, "unused.a"]
            if mode == "resolve":
                command += [str(work / "policy"), "libpinyin.so.15"]
            return subprocess.run(command, capture_output=True, text=True, check=False,
                                  env={**os.environ, "PATH": str(work) + os.pathsep + os.environ["PATH"],
                                       "TEST_NM_INPUT": str(work / "symbols")})

    def test_exact_section_order_includes_unlikely_and_ignores_encounter_order(self):
        data = row("crate::steady", ".text.hash_steady") + row("crate::open", ".text.unlikely.hash_init")
        result = self.invoke("resolve", data, HEADER + "init\tcrate::open\nsteady\tcrate::steady\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout,
                         '.text : {\n  *(".text.unlikely.hash_init")\n  *(".text.hash_steady")\n}\n')
        # The skip count is printed on every resolve, even with no C++ path.
        self.assertIn("function ordering: 0 of 0 C++ policy paths are not in this build; skipped",
                      result.stderr)

    def test_inventory_normalization_matches_generator(self):
        name = "<crate::Store<T>>::open::{closure#1}"
        result = self.invoke("inventory", row(name + "::h0123456789abcdef.llvm.abcdef", ".text.hash"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, name + "\t.text.hash\n")

    def test_inventory_never_treats_stdin_as_a_policy(self):
        # BSD awk names stdin "", matching an unset policy. Force the same
        # collision for every awk by assigning policy=FILENAME per record.
        guard = 'mode != "inventory" && FILENAME == policy'
        program = (HERE / "function-order.awk").read_text()
        self.assertIn(guard, program)
        program = '{ policy = FILENAME }\n' + program
        result = subprocess.run(["awk", "-v", "mode=inventory", program],
                                input=row("crate::open", ".text.hash"),
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "crate::open\t.text.hash\n")

    def test_shared_alias_section_is_emitted_once(self):
        result = self.invoke("resolve", row("crate::open", ".text.same") + row("alias", ".text.same"),
                             HEADER + "init\tcrate::open\nsteady\talias\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.count('*(".text.same")'), 1)

    def test_ambiguous_missing_bundled_and_unsafe_sections_are_rejected(self):
        for data in (row("other", ".text.other"),
                     row("crate::open", ".text.one") + row("crate::open", ".text.two"),
                     row("crate::open", ".text.bundle") + row("inner", ".text.bundle", "0000000000000010"),
                     row("crate::open", '.text.bad"); KEEP(*(.text*))')):
            with self.subTest(data=data):
                result = self.invoke("resolve", data)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, "")

    def test_invalid_policy_never_emits_linker_input(self):
        for policy in (HEADER.replace("libpinyin.so.15", "libzhuyin.so.15"),
                       HEADER + "init\tcrate::open\ninit\tcrate::open\n",
                       HEADER + "steady\tx\ninit\tcrate::open\n",
                       HEADER + "init\t_Rhashed\n"):
            with self.subTest(policy=policy):
                result = self.invoke("resolve", row("crate::open", ".text.one"), policy)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, "")

    def test_bar_inside_demangled_name(self):
        name = "crate::function::<{ 1 | 2 }>"
        result = self.invoke("resolve", row(name, ".text.one"), HEADER + "init\t" + name + "\n")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_distinct_instantiations_resolve_only_the_measured_crate(self):
        name = "crate::open::<T>"
        data = row(name, ".text._RIfakeECsOLD_15oxpinyin_facade")
        data += row(name, ".text.unlikely._RIfakeECsNEW_16oxpinyin_runtime")
        result = self.invoke("resolve", data,
                             HEADER + "init\t" + name + " [instantiating crate: oxpinyin_runtime]\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("oxpinyin_facade", result.stdout)
        self.assertIn("oxpinyin_runtime", result.stdout)

    def test_cs_inside_a_disambiguator_decodes_one_root(self):
        # The tkrzw libzhuyin.a witness: the zhuyin crate's disambiguator
        # 4iCseHRioF7 contains Cs, so two matches decode the same root.
        name = "<alloc::raw_vec::RawVecInner>::finish_grow"
        base = ".text.unlikely._RNvMs4_NtCscdodAO9FK5_5alloc7raw_vecNtB5_11RawVecInner11finish_grow"
        data = row(name, base + "B7_") + row(name, base + "Cs4iCseHRioF7_6zhuyin")
        self.assertEqual(self.invoke("inventory", data).stdout,
                         f"{name}\t{base}B7_\n{name} [instantiating crate: zhuyin]\t{base}Cs4iCseHRioF7_6zhuyin\n")
        result = self.invoke("resolve", data, HEADER + "init\t" + name + " [instantiating crate: zhuyin]\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Cs4iCseHRioF7_6zhuyin", result.stdout)
        self.assertNotIn("B7_", result.stdout)

    def test_two_different_decoded_roots_stay_ambiguous(self):
        self.assertEqual(self.invoke("inventory", row("crate::f", ".text._RfakeCs1_11abcCsZ_3xyz")).stdout,
                         "crate::f\t.text._RfakeCs1_11abcCsZ_3xyz\n")

    def test_recognized_clone_family_includes_every_body(self):
        name = "alloc::boxed::box_new_uninit"
        base = ".text._RNvFake14box_new_uninit"
        result = self.invoke("resolve", row(name, base) + row(name, base + ".1923"),
                             HEADER + "init\t" + name + "\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'*("{base}")', result.stdout)
        self.assertIn(f'*("{base}.1923")', result.stdout)
        self.assertEqual(result.stdout.count('*("'), 2)

    def test_cxx_paths_resolve_and_one_this_build_lacks_is_skipped(self):
        cxx = "_ZN12kyotocabinet6HashDB11accept_implEPKcm"
        clone = "_ZN12_GLOBAL__N_1L4failEP7oxkc_db.isra.0"
        data = row("crate::open", ".text.one") + row(cxx, ".text." + cxx)
        result = self.invoke("resolve", data, HEADER + f"init\tcrate::open\ninit\t{cxx}\ninit\t{clone}\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f'.text : {{\n  *(".text.one")\n  *(".text.{cxx}")\n}}\n')
        self.assertIn("1 of 2 C++ policy paths are not in this build; skipped", result.stderr)
        self.assertIn(f"{cxx}\t.text.{cxx}\n", self.invoke("inventory", data).stdout)

    def test_absent_rust_or_c_path_still_fails(self):
        for name in ("crate::gone", "oxkc_db_new"):
            with self.subTest(name=name):
                result = self.invoke("resolve", row("crate::open", ".text.one"),
                                     HEADER + f"init\tcrate::open\ninit\t{name}\n")
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("unresolved policy path: " + name, result.stderr)
                self.assertEqual(result.stdout, "")

    def test_raw_rust_symbols_are_neither_policy_rows_nor_inventory(self):
        legacy = "_ZN4core3ptr13drop_in_place17h0123456789abcdefE"
        for name in (legacy, legacy + ".llvm.1234", "_RNvCs1234_5crate4open"):
            with self.subTest(name=name):
                result = self.invoke("resolve", row("crate::open", ".text.one"),
                                     HEADER + f"init\tcrate::open\ninit\t{name}\n")
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertEqual(self.invoke("inventory", row(name, ".text.raw")).stdout, "")


class CapabilityTests(unittest.TestCase):
    def supported(self, version, help_text):
        with scratch() as work:
            compiler = work / "compiler"
            compiler.write_text(
                '#!/bin/sh\nfor arg; do\ncase "$arg" in\n'
                '  --version) printf "%s\\n" "$TEST_LINKER_VERSION"; exit 0;;\n'
                '  --help) printf "%s\\n" "$TEST_LINKER_HELP"; exit 0;;\n'
                'esac\ndone\nexit 1\n')
            compiler.chmod(0o755)
            command = [sys.executable, '-X', 'lazy_imports=normal', '-c',
                       'import sys; from tools.dev.packaging import Tools; '
                       'from tools.packaging.function_order import supported; '
                       'sys.exit(0 if supported(sys.argv[1], "unused.a", sys.argv[2], run=Tools().capture) else 1)',
                       str(compiler), str(work)]
            return subprocess.run(command, capture_output=True, text=True, check=False,
                                  env={**os.environ, "TEST_LINKER_VERSION": version,
                                       "TEST_LINKER_HELP": help_text})

    def test_bfd_feature_is_required(self):
        self.assertEqual(self.supported("GNU ld (GNU Binutils) 2.47",
                                        "  --section-ordering-file FILE").returncode, 0)
        for version, help_text in (("GNU ld (GNU Binutils) 2.42", "old help"),
                                   ("GNU gold (GNU Binutils) 1.16", " --section-ordering-file FILE"),
                                   ("LLD 20", " --section-ordering-file FILE")):
            with self.subTest(version=version):
                self.assertNotEqual(self.supported(version, help_text).returncode, 0)


class LinkedObjectTests(unittest.TestCase):
    def test_policy_reader_reuses_the_packaging_validation(self):
        with scratch() as work:
            policy = work / 'policy'
            command = [sys.executable, '-X', 'lazy_imports=normal', str(ROOT / 'tools/dev/__main__.py'), 'packaging', 'function-order', 'policy', str(policy), 'libpinyin.so.15']
            policy.write_text(HEADER + 'init\ta\nsteady\tb\n')
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, 'a\nb\n')
            for text in ('', HEADER, HEADER + 'init\ta\ninit\ta\n',
                         HEADER + 'steady\tb\ninit\ta\n',
                         HEADER.replace('pinyin', 'zhuyin') + 'init\ta\n'):
                policy.write_text(text)
                with self.subTest(text=text):
                    self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)

    def check(self, entries, names=('a', 'b'), *, start='100', soname='libpinyin.so.15', raw=None):
        records = ''.join(row(name, '.text', f'{address:016x}') for name, address in entries)
        raw_records = ''.join(row(name, '.text', f'{address:016x}') for name, address in (raw or entries))

        def command(arguments):
            if arguments[0] == 'bash':
                return '\n'.join(names) + '\n'
            if arguments[0] == 'readelf':
                if arguments[1] == '-dW':
                    return f' (SONAME) Library soname: [{soname}]\n'
                return f' [13] .text PROGBITS {start} 00100 100 00 AX 0 0 16\n'
            return records if '--demangle=rust' in arguments else raw_records

        with mock.patch.dict(ORDER['verify'].__globals__, command=command):
            return ORDER['verify'](LIBRARY, Path('/policy'), 'pinyin',
                policy_reader=lambda *args, **kwargs: pool.Completed(0, ('\n'.join(names) + '\n').encode(), b''))

    def test_ordered_passes_and_unordered_fails(self):
        self.assertEqual(self.check([('a', 0x100), ('b', 0x120), ('crt', 0x140)])['paths'], 2)
        for entries in ([('crt', 0x100), ('a', 0x120), ('b', 0x140)],
                        [('b', 0x100), ('a', 0x120)],
                        [('a', 0x100), ('crt', 0x120), ('b', 0x140)]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                self.check(entries)

    def test_prefix_and_every_path_are_required(self):
        with self.assertRaisesRegex(ValueError, 'start of .text'):
            self.check([('a', 0x120), ('b', 0x140)])
        with self.assertRaisesRegex(ValueError, 'missing.*b'):
            self.check([('a', 0x100)])
        with self.assertRaisesRegex(ValueError, 'stripped'):
            self.check([])
        with self.assertRaisesRegex(ValueError, 'SONAME'):
            self.check([], soname='libzhuyin.so.15')

    def test_clone_bodies_are_contiguous_and_aliases_can_share_an_address(self):
        self.check([('a', 0x100), ('a', 0x110), ('b', 0x120), ('b', 0x130)])
        self.check([('a', 0x100), ('b', 0x100)])
        for entries in ([('a', 0x100), ('b', 0x120), ('a', 0x140)],
                        [('a', 0x100), ('b', 0x120), ('crt', 0x130), ('b', 0x140)]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                self.check(entries)

    def test_instantiating_crates_are_distinct(self):
        names = ('crate::f [instantiating crate: oxpinyin_store]',
                 'crate::f [instantiating crate: oxpinyin_runtime]')
        self.check([('crate::f', 0x100), ('crate::f', 0x120)], names,
                   raw=[('_RfakeCsOLD_14oxpinyin_store', 0x100),
                        ('_RfakeCsNEW_16oxpinyin_runtime', 0x120)])

    def test_cxx_path_the_object_lacks_is_skipped_but_rust_and_c_are_required(self):
        cxx = '_ZN12kyotocabinet6HashDB3getEv'
        self.assertEqual(self.check([('a', 0x100), ('b', 0x120)], ('a', cxx, 'b'))['paths'], 2)
        self.assertEqual(self.check([('a', 0x100), (cxx, 0x110), ('b', 0x120)], ('a', cxx, 'b'))['paths'], 3)
        with self.assertRaisesRegex(ValueError, 'out of order'):
            self.check([('a', 0x100), ('b', 0x110), (cxx, 0x120)], ('a', cxx, 'b'))
        with self.assertRaisesRegex(ValueError, 'missing.*oxkc_db_new'):
            self.check([('a', 0x100), ('b', 0x120)], ('a', 'oxkc_db_new', 'b'))

    def test_two_unrelated_bodies_cannot_masquerade_as_clones(self):
        with self.assertRaisesRegex(ValueError, 'ambiguous policy path'):
            self.check([('a', 0x100), ('a', 0x110), ('b', 0x120)],
                       raw=[('body_one', 0x100), ('body_two', 0x110), ('b', 0x120)])

    def test_each_arm_is_checked_and_result_is_reported(self):
        arms = {arm: {facade: LIBRARY for facade in ('pinyin', 'zhuyin')}
                for arm in ('base', 'pr', 'control', 'pin')}
        args = argparse.Namespace(function_order=['base/pinyin=/old-policy'], unordered_arm=[])
        with mock.patch.dict(ORDER['check_arms'].__globals__, verify=mock.Mock(return_value={}),
                             library_backend=lambda *_: 'bdb',
                             linker_description=lambda *_: 'GNU ld 2.47'), \
                mock.patch('sys.stdout', new_callable=io.StringIO) as output:
            result = ORDER['check_arms'](args, arms, references=('pin',))
            calls = ORDER['check_arms'].__globals__['verify'].call_args_list
        self.assertEqual(len(calls), 6)
        self.assertEqual(calls[0].args[1], Path('/old-policy'))
        self.assertEqual(len(output.getvalue().splitlines()), 8)
        self.assertEqual(result['control']['zhuyin']['function_order'], 'ordered')
        self.assertEqual(result['pin']['pinyin']['function_order'], 'reference')

    def test_refusal_names_arm_and_linker_without_trusting_linker_support(self):
        with mock.patch.dict(ORDER['check_arms'].__globals__, verify=mock.Mock(side_effect=ValueError('unordered')),
                             library_backend=lambda *_: 'bdb',
                             linker_description=lambda *_: 'GNU ld 2.47'), \
                mock.patch('sys.stdout', new_callable=io.StringIO) as output:
            with self.assertRaisesRegex(ValueError, 'arm pr/pinyin refused.*GNU ld 2.47'):
                ORDER['check_arms'](argparse.Namespace(), {'pr': {'pinyin': LIBRARY}})
        self.assertEqual(json.loads(output.getvalue())['function_order'], 'refused')

    def test_one_refusal_still_reports_the_other_arms_before_stopping(self):
        arms = {arm: {'pinyin': LIBRARY} for arm in ('base', 'pr', 'control', 'pin')}
        verify = mock.Mock(side_effect=[ValueError('unordered'), {}, ValueError('fallback')])
        with mock.patch.dict(ORDER['check_arms'].__globals__, verify=verify,
                             library_backend=lambda *_: 'bdb',
                             linker_description=lambda *_: 'GNU ld 2.42'), \
                mock.patch('sys.stdout', new_callable=io.StringIO) as output:
            with self.assertRaisesRegex(ValueError, 'arm base/pinyin refused') as failure:
                ORDER['check_arms'](argparse.Namespace(), arms, references=('pin',))
        self.assertEqual(verify.call_count, 3)
        rows = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual([row['function_order'] for row in rows],
                         ['refused', 'ordered', 'refused', 'reference'])
        self.assertIn('arm control/pinyin refused', str(failure.exception))

    def test_unordered_experiment_must_be_explicit_and_cannot_override_a_policy(self):
        args = argparse.Namespace(unordered_arm=['base'], function_order=[])
        with mock.patch.dict(ORDER['check_arms'].__globals__, verify=mock.Mock(side_effect=AssertionError),
                             linker_description=lambda *_: 'GNU ld 2.42'), \
                mock.patch('sys.stdout', new_callable=io.StringIO):
            result = ORDER['check_arms'](args, {'base': {'pinyin': LIBRARY}})
        self.assertEqual(result['base']['pinyin']['function_order'], 'intentional-unordered')
        for flags in (['base/pinyin=/policy'], ['typo/pinyin=/policy']):
            args.function_order = flags
            with self.assertRaises(ValueError):
                ORDER['check_arms'](args, {'base': {'pinyin': LIBRARY}})
        args = argparse.Namespace(unordered_arm=['typo'], function_order=[])
        with self.assertRaisesRegex(ValueError, 'unique non-reference'):
            ORDER['check_arms'](args, {'base': {'pinyin': LIBRARY}})


class BackendPolicyTests(unittest.TestCase):
    def test_legacy_nightly_aliases_select_bdb_and_refuse_foreign_backends(self):
        for facade in ('pinyin', 'zhuyin'):
            alias = HERE / 'function-order' / f'lib{facade}.order'
            self.assertTrue(alias.is_symlink())
            self.assertEqual(alias.resolve(), HERE / 'function-order/bdb' / f'lib{facade}.order')
            ORDER['check_policy_backend'](alias, 'bdb')
            for backend in ('kc', 'tkrzw'):
                with self.assertRaisesRegex(ValueError, 'policy backend bdb does not match'):
                    ORDER['check_policy_backend'](alias, backend)

    def test_backend_requires_one_recognized_needed_library(self):
        for backend, library in (('bdb', 'libdb-5.3.so'), ('kc', 'libkyotocabinet.so.16'),
                                 ('tkrzw', 'libtkrzw.so.1')):
            with self.subTest(backend=backend), mock.patch.dict(
                    ORDER['library_backend'].__globals__,
                    command=lambda *_: f'(NEEDED) Shared library: [{library}]\n'):
                self.assertEqual(ORDER['library_backend'](LIBRARY), backend)
        for needed in ('libc.so.6', 'libdb-5.3.so]\n(NEEDED) [libtkrzw.so.1',
                       'libtkrzw.so.1]\n(NEEDED) [libtkrzw.so.2'):
            with self.subTest(needed=needed), mock.patch.dict(
                    ORDER['library_backend'].__globals__,
                    command=lambda *_: f'(NEEDED) [{needed}]\n'):
                with self.assertRaisesRegex(ValueError, 'exactly one backend'):
                    ORDER['library_backend'](LIBRARY)

    def test_defaults_follow_each_objects_backend_and_foreign_overrides_fail(self):
        for backend in ('bdb', 'kc', 'tkrzw'):
            with self.subTest(backend=backend), mock.patch.dict(
                    ORDER['check_arms'].__globals__, library_backend=lambda *_: backend,
                    verify=mock.Mock(return_value={}), linker_description=lambda *_: 'GNU ld 2.47'), \
                    mock.patch('sys.stdout', new_callable=io.StringIO):
                result = ORDER['check_arms'](argparse.Namespace(), {'pr': {'pinyin': LIBRARY}})
                self.assertEqual(result['pr']['pinyin']['backend'], backend)
                self.assertEqual(result['pr']['pinyin']['policy'],
                                 str(HERE / 'function-order' / backend / 'libpinyin.order'))
                foreign = 'kc' if backend == 'bdb' else 'bdb'
                args = argparse.Namespace(function_order=[
                    f'pr/pinyin={HERE}/function-order/{foreign}/libpinyin.order'])
                with self.assertRaisesRegex(ValueError, 'policy backend .* does not match'):
                    ORDER['check_arms'](args, {'pr': {'pinyin': LIBRARY}})

    def test_foreign_policy_symlink_is_rejected(self):
        with scratch() as work:
            target = work / 'function-order/bdb/libpinyin.order'
            target.parent.mkdir(parents=True)
            target.write_text(HEADER + 'init\ta\n')
            link = work / 'alias.order'
            link.symlink_to(target)
            with self.assertRaisesRegex(ValueError, 'policy backend bdb does not match kc'):
                ORDER['check_policy_backend'](link, 'kc')


def measure(recipe, *arguments):
    """Run `python -m tools.dev measure RECIPE ARGUMENTS` in a child, as the registry and the images do."""
    return subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'measure', recipe,
                           *map(str, arguments)], capture_output=True, text=True, cwd=ROOT)


class MeasurementCallerTests(unittest.TestCase):
    def test_input_library_tools_refuse_before_creating_output_or_starting_a_worker(self):
        with scratch() as work:
            library = work / 'unordered.so'
            library.write_bytes(b'not an ordered ELF')
            recipes = [('rss-diag', 'S', ['--lp', library, '--ox', library, '--data', work, '--out', work / 'output']),
                       ('system-import-profile', 'ox', [library, library, work, work / 'output', '--cold-open'])]
            for recipe, arm, arguments in recipes:
                with self.subTest(recipe=recipe):
                    result = measure(recipe, *arguments)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f'arm {arm}/pinyin refused', result.stderr)
                    self.assertFalse((work / 'output').exists())


def load_profile_module():
    spec = importlib.util.spec_from_file_location('system_import_profile',
                                                  ROOT / 'tools/bisection/system_import_profile.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PROFILE = load_profile_module()


class FakeAdmission:
    """HeavyAdmission stand-in: records its lifecycle and admits a fixed worker count."""
    workers = 2
    events = None

    def __init__(self, label, env, out=None, runner=False, prepare_heavy_env=None):
        self.env = {pool.POOL_WORKERS: str(self.workers)}
        self.slot = type('Slot', (), {'pass_fds': staticmethod(lambda: ()), 'handoff': staticmethod(lambda: None)})()
        self.events.append('admission')

    def try_acquire(self):
        return True

    def close(self):
        self.events.append('close')


class SystemImportProfileTests(unittest.TestCase):
    """The independent collections: order, retries, validation, runtime check; no Valgrind, /proc or CLOCK_BOOTTIME.

    The pool's own rules (admission bound, stop on failure, reaping, cancellation) are tested in
    tools/dev/pool.test.py; here the collections go through that pool with a fake admission.
    """
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='system-import-profile-test-')
        self.work = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        self.events, self.lock = [], threading.Lock()
        FakeAdmission.events, FakeAdmission.workers = self.events, 2
        # The host readers a collection consults: the boot-minus-monotonic clock offset (Linux only)
        # and /proc/loadavg. Neither exists on macOS.
        self.offsets = lambda: 0
        for patcher in (mock.patch('tools.dev.slot.HeavyAdmission', FakeAdmission),
                        mock.patch.object(PROFILE, 'suspend_offset', lambda: self.offsets()),
                        mock.patch.object(PROFILE, 'load_average', lambda: '0.25')):
            patcher.start()
            self.addCleanup(patcher.stop)
        PROFILE.slot.CANCELLED.clear()
        self.addCleanup(PROFILE.slot.CANCELLED.clear)
        self.plan = dict(pin=Path('/pin.so'), ox=Path('/ox.so'), data=Path('/data'), work=self.work, workload='30')

    def emitted(self, jobs):
        lines = []
        PROFILE.run_collections(self.plan, jobs, lambda side, mode, report: lines.append((side, mode, report)))
        return lines

    def test_jobs_two_caps_concurrent_collections_despite_larger_admission(self):
        FakeAdmission.workers = 6
        active, peak, completed = 0, 0, []
        together = threading.Event()

        def collect(side, mode, plan, environment, context):
            nonlocal active, peak
            with self.lock:
                active += 1
                peak = max(peak, active)
                if active == 2:
                    together.set()
            self.assertTrue(together.wait(5), 'two collections did not overlap')
            time.sleep(.02)
            with self.lock:
                active -= 1
                completed.append((side, mode))
            return [f'{side} {mode}']

        with mock.patch.object(PROFILE, 'collect', collect):
            status, output, checks, _ = self.run_main(self.good_outcomes(), '--jobs', '2')
        order = [(side, mode) for side in PROFILE.SIDES for mode in PROFILE.MODES]
        self.assertEqual(status, 0)
        self.assertEqual(peak, 2)
        self.assertCountEqual(completed, order)
        self.assertEqual(output.splitlines(), [f'{side} {mode}' for side, mode in order])
        self.assertEqual([side for _, _, side, _ in checks], ['pin', 'ox'])

    def test_every_collection_gets_the_same_captured_environment_in_both_modes(self):
        runs = {}

        def collect(side, mode, plan, environment, context):
            runs.setdefault(self.mode, []).append(environment)
            return []
        with mock.patch.object(PROFILE, 'collect', collect), mock.patch.dict(os.environ, {'PROBE': 'x'}):
            for self.mode, jobs in (('serial', 1), ('pooled', None)):
                self.emitted(jobs)
            for environments in runs.values():
                self.assertEqual(len(environments), 6)
                for environment in environments:
                    test_support.assert_same_environment(self, environment, dict(os.environ))

    def launcher(self, outcomes):
        """Record each launch and return the next outcome for its side and mode."""
        calls = []

        def launch(context, argv, env=None, **kwargs):
            user = Path(argv[-2])
            side, mode = user.name.split('-user-')[0].split('-')
            calls.append((side, mode, user.name, threading.get_ident()))
            queued = outcomes[(side, mode)]
            status, out, err, create = queued.pop(0) if len(queued) > 1 else queued[0]
            if create:
                user.mkdir(parents=True, exist_ok=True)
                (user / 'user.conf').write_text('')
            return pool.Completed(status, out, err)
        return launch, calls

    GOOD = {'callgrind': (b'summary: 4\nsummary: 5\nsummary: 6\nsummary: 7\n'),
            'heap': b'mem_heap_B=10\nmem_heap_B=30\nmem_heap_B=20\n', 'pages': b'mem_heap_B=7\n'}

    def good_outcomes(self):
        return {(side, mode): [(0, b'', self.GOOD[mode], True)] for side in PROFILE.SIDES for mode in PROFILE.MODES}

    def collect(self, outcomes, side='pin', mode='callgrind'):
        launch, calls = self.launcher(outcomes)
        with mock.patch.object(pool.UnitContext, 'launch', launch):
            return PROFILE.collect(side, mode, self.plan, {}, pool.UnitContext('x', threading.Event())), calls

    def test_users_are_private_per_side_mode_and_attempt_and_retries_follow_their_own_discard(self):
        FakeAdmission.workers = 6
        launch, calls = self.launcher(self.good_outcomes())
        # Only pin heap overlaps a suspend, once: the offset jumps during its first attempt.
        local = threading.local()
        real_collect = PROFILE.collect

        def collect(side, mode, plan, environment, context):
            local.offsets = iter([0, 5_000_000, 5_000_000, 5_000_000]) if (side, mode) == ('pin', 'heap') else None
            return real_collect(side, mode, plan, environment, context)
        self.offsets = lambda: next(local.offsets) if local.offsets else 0
        with mock.patch.object(pool.UnitContext, 'launch', launch), mock.patch.object(PROFILE, 'collect', collect):
            reports = self.emitted(None)
        names = [name for _, _, name, _ in calls]
        self.assertEqual(len(names), len(set(names)))
        self.assertEqual(sorted(names), sorted(
            [f'{side}-{mode}-user-1' for side in PROFILE.SIDES for mode in PROFILE.MODES] + ['pin-heap-user-2']))
        heap = [(name, thread) for side, mode, name, thread in calls if (side, mode) == ('pin', 'heap')]
        self.assertEqual([name for name, _ in heap], ['pin-heap-user-1', 'pin-heap-user-2'])
        self.assertEqual(heap[0][1], heap[1][1])
        self.assertTrue(reports[1][2][0].startswith('DISCARD pin heap: suspend_delta_ns='))
        self.assertTrue(reports[1][2][1].startswith('pin heap: utc='))
        self.assertEqual(reports[1][2][-1], 'pin peak heap: 30 bytes')

    def test_six_suspend_overlapping_attempts_end_the_run(self):
        values = iter([0, 9_000_000] * 6)
        self.offsets = values.__next__
        with self.assertRaises(PROFILE.Failure) as raised:
            self.collect(self.good_outcomes())
        self.assertEqual(raised.exception.message, 'repeated suspend-overlapping runs')
        self.assertEqual(len(raised.exception.lines), 6)

    def test_output_validation_is_strict(self):
        cases = {
            'status': ((1, b'', self.GOOD['callgrind'], True), 'pin callgrind: failed workload or user-store open'),
            'stdout': ((0, b'x', self.GOOD['callgrind'], True), 'pin callgrind: failed workload or user-store open'),
            'marker': ((0, b'', b'no-user-state\n' + self.GOOD['callgrind'], True),
                       'pin callgrind: failed workload or user-store open'),
            'user.conf': ((0, b'', self.GOOD['callgrind'], False), 'pin callgrind: failed workload or user-store open'),
            'dumps': ((0, b'', b'summary: 4\n', True), 'pin: expected 4 Callgrind dumps, got 1'),
        }
        for name, (outcome, message) in cases.items():
            with self.subTest(name):
                shutil.rmtree(self.work / 'pin-callgrind-user-1', ignore_errors=True)
                outcomes = self.good_outcomes()
                outcomes[('pin', 'callgrind')] = [outcome]
                with self.assertRaises(PROFILE.Failure) as raised:
                    self.collect(outcomes)
                self.assertEqual(raised.exception.message, message)
        self.assertEqual(PROFILE.report('pin', 'callgrind', '--cold-open', b'summary: 9\n'),
                         ['pin cold-open: 9 instructions', 'pin total: 9 instructions'])
        with self.assertRaisesRegex(PROFILE.Failure, 'missing Massif heap snapshots'):
            PROFILE.report('ox', 'pages', '30', b'')

    def run_main(self, outcomes, *arguments, runtime_check=None, workload='30'):
        launch, calls = self.launcher(outcomes)
        checks = []
        runtime = runtime_check or (lambda cell, runner, side, obj, **kw: checks.append((cell, runner, side, str(obj))))
        output = io.StringIO()
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': runtime}), \
                mock.patch('sys.stdout', output):
            status = PROFILE.main([*arguments, '--', '/pin.so', '/ox.so', '/data', str(self.work), workload])
        return status, output.getvalue(), checks, calls

    def test_main_prints_every_side_and_mode_in_the_serial_order_and_checks_both_runtimes_first(self):
        status, output, checks, calls = self.run_main(self.good_outcomes(), '--jobs', '1')
        self.assertEqual(status, 0)
        labels = [line.split(':')[0] for line in output.splitlines()]
        self.assertEqual(labels, ['pin callgrind', 'pin open', 'pin import', 'pin save', 'pin close', 'pin total',
                                  'pin heap', 'pin peak heap', 'pin pages', 'pin peak pages',
                                  'ox callgrind', 'ox open', 'ox import', 'ox save', 'ox close', 'ox total',
                                  'ox heap', 'ox peak heap', 'ox pages', 'ox peak pages'])
        self.assertEqual(checks, [('auto', 'run-system-import-profile.sh', 'pin', '/pin.so'),
                                  ('auto', 'run-system-import-profile.sh', 'ox', '/ox.so')])
        self.assertEqual([name for _, _, name, _ in calls], [f'{side}-{mode}-user-1' for side in PROFILE.SIDES
                                                           for mode in PROFILE.MODES])

    def test_cold_open_is_a_workload_not_an_option(self):
        outcomes = self.good_outcomes()
        outcomes[('pin', 'callgrind')] = [(0, b'', b'summary: 9\n', True)]
        outcomes[('ox', 'callgrind')] = [(0, b'', b'summary: 8\n', True)]
        _, output, _, _ = self.run_main(outcomes, '--jobs', '1', workload='--cold-open')
        self.assertIn('pin cold-open: 9 instructions', output)
        self.assertIn('ox total: 8 instructions', output)

    def test_pooled_main_prints_the_same_lines_as_serial_apart_from_the_run_time_fields(self):
        FakeAdmission.workers = 6

        def normal(text):
            return re.sub(r'utc=\S+ load1=\S+ suspend_delta_ns=-?\d+', 'RUN', text)
        serial = self.run_main(self.good_outcomes(), '--jobs', '1')[1]
        pooled = self.run_main(self.good_outcomes())[1]
        self.assertEqual(normal(serial), normal(pooled))
        self.assertEqual(len(pooled.splitlines()), 20)

    def test_an_unsound_runtime_stops_before_any_collection_starts(self):
        def refuse(cell, runner, side, obj, **kwargs):
            if side == 'ox':
                raise RuntimeError('runtime refused')
        launch, calls = self.launcher(self.good_outcomes())
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': refuse}):
            with self.assertRaisesRegex(RuntimeError, 'runtime refused'):
                PROFILE.main(['--', '/pin.so', '/ox.so', '/data', str(self.work), '30'])
        self.assertEqual(calls, [])

    def test_a_failure_prints_the_finished_lines_then_stderr_then_exits_with_its_message(self):
        outcomes = self.good_outcomes()
        outcomes[('pin', 'heap')] = [(1, b'', b'valgrind said no', True)]
        launch, _ = self.launcher(outcomes)
        out, err = io.StringIO(), io.BytesIO()
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': lambda *a, **k: None}), \
                mock.patch('sys.stdout', out), mock.patch.object(PROFILE.sys, 'stderr', io.TextIOWrapper(err)) as text:
            with self.assertRaises(SystemExit) as raised:
                PROFILE.main(['--jobs', '1', '--', '/pin.so', '/ox.so', '/data', str(self.work), '30'])
            text.flush()
        self.assertEqual(raised.exception.code, 'pin heap: failed workload or user-store open')
        self.assertIn('pin total:', out.getvalue())
        self.assertNotIn('ox ', out.getvalue())
        self.assertEqual(err.getvalue(), b'valgrind said no')

    def test_a_pooled_failure_reports_the_earliest_collection_and_prints_those_before_it(self):
        FakeAdmission.workers = 6
        outcomes = self.good_outcomes()
        outcomes[('pin', 'pages')] = [(1, b'', b'pin pages broke', True)]
        outcomes[('ox', 'heap')] = [(1, b'', b'ox heap broke', True)]
        launch, _ = self.launcher(outcomes)
        out = io.StringIO()
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(PROFILE.runpy, 'run_path', lambda path: {'check': lambda *a, **k: None}), \
                mock.patch('sys.stdout', out), mock.patch.object(PROFILE.sys, 'stderr', io.TextIOWrapper(io.BytesIO())):
            with self.assertRaises(SystemExit) as raised:
                PROFILE.main(['--', '/pin.so', '/ox.so', '/data', str(self.work), '30'])
        self.assertEqual(raised.exception.code, 'pin pages: failed workload or user-store open')
        self.assertIn('pin peak heap:', out.getvalue())
        self.assertNotIn('ox ', out.getvalue())
        self.assertEqual(self.events[-1], 'close')

    def test_the_recipe_takes_jobs_before_or_after_the_ordering_options(self):
        with scratch() as work:
            library = work / 'unordered.so'
            library.write_bytes(b'not an ordered ELF')
            for tail in (['--jobs', '1'], ['30', '--jobs', '2']):
                with self.subTest(tail=tail):
                    result = measure('system-import-profile', library, library, work, work / 'output', *tail)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('arm ox/pinyin refused', result.stderr)
                    self.assertNotIn('unknown function-order argument', result.stderr)
                    self.assertFalse((work / 'output').exists())
            result = measure('system-import-profile', library, library, work, work / 'output', '--jobs')
            self.assertEqual(result.returncode, 2)
            self.assertIn('--jobs needs a count', result.stderr)


class RelinkTests(unittest.TestCase):
    def prepare(self, work):
        scripts = {
            # The relinker is intentionally Linux-only. Its command-transport
            # fixtures simulate that packaging host on macOS as well.
            "uname": "#!/bin/sh\nprintf 'Linux\\n'\n",
            "compiler": r'''#!/bin/sh
for arg; do
  case "$arg" in
    --version)
      printf 'query\n' >> "$TEST_QUERIES"
      printf '%s\n' "$TEST_LINKER_VERSION"; exit 0;;
    --help) printf '%s\n' "$TEST_LINKER_HELP"; exit 0;;
  esac
done
printf '%s\000' "$@" > "$TEST_LINK_ARGS"
out=''
previous=''
for arg; do
  if [ "$previous" = '-o' ]; then out=$arg; fi
  previous=$arg
done
printf 'synthetic linked bytes\n' > "$out"
exit "${TEST_LINK_EXIT:-0}"
''',
            "nm": r'''#!/bin/sh
case "$1" in
  -D)
    printf '00000000 A LIBPINYIN\n00000001 T pinyin_init@@LIBPINYIN\n'
    if [ "${TEST_EXTRA_EXPORT:-0}" = 1 ]; then printf '00000002 T leak@@LIBPINYIN\n'; fi;;
  *)
    [ "${TEST_NM_EXIT:-0}" = 0 ] || exit 1
    cat "$TEST_NM_INPUT";;
esac
''',
            "readelf": r'''#!/bin/sh
printf ' 0x000000000000000e (SONAME) Library soname: [libpinyin.so.15]\n'
''',
        }
        for name, contents in scripts.items():
            (work / name).write_text(contents)
            (work / name).chmod(0o755)
        (work / "symbols").write_text(row("crate::open", ".text.unlikely._Rhash"))
        (work / "policy").write_text(HEADER + "init\tcrate::open\n")
        (work / "version").write_text("LIBPINYIN {\n global:\n pinyin_init;\n local:\n *;\n};\n")
        (work / "library.a").write_bytes(b"synthetic archive")
        return {**os.environ, "PATH": str(work) + os.pathsep + os.environ["PATH"],
                "TEST_NM_INPUT": str(work / "symbols"), "TEST_LINK_ARGS": str(work / "args"),
                "TEST_QUERIES": str(work / "queries"), "TEST_LINKER_VERSION": "GNU ld (GNU Binutils) 2.47",
                "TEST_LINKER_HELP": "  --section-ordering-file FILE", "TEST_LINK_EXIT": "0",
                "TEST_EXTRA_EXPORT": "0", "TEST_NM_EXIT": "0", "OXPINYIN_FUNCTION_ORDER_POLICY": ""}

    def relink(self, work, environment, policy=None, script=None, features=None):
        destination = work / "libpinyin.so.15.0.0"
        destination.write_bytes(b"original installed bytes")
        driver = 'import sys\nfrom pathlib import Path\nfrom tools.dev import packaging\nclass FixtureTools(packaging.Tools):\n def heavy(self, argv, **options): self.run(argv, **options)\nsys.exit(packaging.main(sys.argv[2:], tools=FixtureTools(root=Path(sys.argv[1]))))'
        command = [sys.executable, '-X', 'lazy_imports=normal', '-c', driver,
                   str(script.parent.parent.parent if script else ROOT), 'relink-versioned', "--staticlib", str(work / "library.a"),
                   "--ver", str(work / "version"), "--soname", "libpinyin.so.15", "--dest", str(destination),
                   "--cc", str(work / "compiler")]
        if policy is not None:
            command += ["--function-order", str(policy)]
        if features is not None:
            command += ["--features", features]
        # The link arguments are those of THIS run: a refusal that never
        # reaches the link leaves none, and the caller sees None.
        (work / "args").unlink(missing_ok=True)
        result = subprocess.run(command, capture_output=True, text=True, check=False, env=environment)
        arguments = None
        if (work / "args").exists():
            arguments = (work / "args").read_bytes().split(b"\0")[:-1]
            arguments[arguments.index(b"-o") + 1] = b"OUTPUT"
        produced = destination.read_bytes()
        self.assertEqual(list(work.glob("*.relink.*")), [], "relink scratch leaked")
        return result, arguments, produced

    def test_explicit_empty_skips_query_and_fallback_preserves_link_arguments(self):
        with scratch() as work:
            environment = self.prepare(work)
            baseline, base_args, base_bytes = self.relink(work, environment)
            self.assertEqual(baseline.returncode, 0, baseline.stderr)
            self.assertFalse((work / "queries").exists())
            self.assertIn("disabled by explicit opt-out", baseline.stderr)
            for version in ("GNU gold (GNU Binutils) 1.16", "LLD 20"):
                fallback, arguments, data = self.relink(
                    work, {**environment, "TEST_LINKER_VERSION": version}, work / "policy")
                self.assertEqual(fallback.returncode, 0, fallback.stderr)
                self.assertEqual(arguments, base_args)
                self.assertEqual(data, base_bytes)
                self.assertIn("linker does not support", fallback.stderr)
                self.assertIn("original layout", fallback.stderr)
            fallback, arguments, data = self.relink(
                work, {**environment, "TEST_LINKER_HELP": "no ordering option"}, work / "policy")
            self.assertEqual(fallback.returncode, 0, fallback.stderr)
            self.assertEqual(arguments, base_args)
            self.assertEqual(data, base_bytes)
            self.assertIn("linker does not support", fallback.stderr)

    def test_default_selects_its_soname_policy_and_unsupported_linker_falls_back(self):
        with scratch() as work:
            environment = self.prepare(work)
            _, base_args, base_bytes = self.relink(work, environment)
            scripts = work / "tools/packaging"
            (scripts / "function-order/bdb").mkdir(parents=True)
            shutil.copyfile(work / "policy", scripts / "function-order/bdb/libpinyin.order")
            environment.pop("OXPINYIN_FUNCTION_ORDER_POLICY")
            ordered, args, _ = self.relink(work, environment, script=scripts / "relink-versioned.sh")
            self.assertEqual(ordered.returncode, 0, ordered.stderr)
            self.assertIn(b"--section-ordering-file", args)
            for linker in ("GNU gold (GNU Binutils) 1.16", "LLD 20"):
                fallback, args, data = self.relink(
                    work, {**environment, "TEST_LINKER_VERSION": linker}, script=scripts / "relink-versioned.sh")
                self.assertEqual(fallback.returncode, 0, fallback.stderr)
                self.assertEqual(args, base_args)
                self.assertEqual(data, base_bytes)

    def test_stale_policy_fails_before_the_link(self):
        # A policy that does not describe the archive is an error on every
        # linker, before any link command runs: the installed object is
        # untouched, the resolver's own message names the path, and the
        # error points at the regeneration procedure. Neither an unsupported
        # linker nor a failing demangler turns that error into a fallback.
        with scratch() as work:
            environment = self.prepare(work)
            (work / "policy").write_text(HEADER + "init\tmissing\n")
            cases = {"supported linker": environment,
                     "unsupported linker": {**environment, "TEST_LINKER_VERSION": "LLD 20"},
                     "demangler failure": {**environment, "TEST_NM_EXIT": "1"}}
            for case, case_environment in cases.items():
                with self.subTest(case=case):
                    result, arguments, data = self.relink(work, case_environment, work / "policy")
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIsNone(arguments, "the link ran after a policy refusal")
                    self.assertEqual(data, b"original installed bytes")
                    self.assertIn("function ordering: unresolved policy path: missing", result.stderr)
                    self.assertIn("Regeneration", result.stderr)
                    self.assertNotIn("original layout", result.stderr)

    def test_missing_named_policy_is_an_error_not_a_fallback(self):
        with scratch() as work:
            environment = self.prepare(work)
            result, arguments, data = self.relink(work, environment, work / "absent.order")
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertEqual(data, b"original installed bytes")
            self.assertIn("policy not found", result.stderr)
            self.assertNotIn("original layout", result.stderr)

    def test_missing_backend_directory_fails_without_consulting_bdb(self):
        with scratch() as work:
            environment = self.prepare(work)
            scripts = work / "tools/packaging"
            (scripts / "function-order/bdb").mkdir(parents=True)
            # A policy no backend's archive satisfies: the BDB default must
            # not be consulted for the other backends at all.
            (scripts / "function-order/bdb/libpinyin.order").write_text(HEADER + "init\tbdb_only\n")
            environment.pop("OXPINYIN_FUNCTION_ORDER_POLICY")
            for backend in ("kyotocabinet", "tkrzw"):
                with self.subTest(backend=backend):
                    opted_out, base_args, base_bytes = self.relink(
                        work, {**environment, "OXPINYIN_FUNCTION_ORDER_POLICY": ""},
                        script=scripts / "relink-versioned.sh", features=backend)
                    self.assertEqual(opted_out.returncode, 0, opted_out.stderr)
                    result, arguments, data = self.relink(
                        work, environment, script=scripts / "relink-versioned.sh", features=backend)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIsNone(arguments)
                    self.assertEqual(data, b'original installed bytes')
                    self.assertIn('policy not found', result.stderr)
                    self.assertNotIn('unresolved policy path: bdb_only', result.stderr)
                    self.assertNotIn('original layout', result.stderr)
                    self.assertFalse((work / "queries").exists())
            # The BDB default is still consulted, and still strict.
            result, arguments, _ = self.relink(work, environment, script=scripts / "relink-versioned.sh",
                                               features="bdb")
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertIn("unresolved policy path: bdb_only", result.stderr)

    def test_named_policy_applies_on_every_backend(self):
        with scratch() as work:
            environment = self.prepare(work)
            for backend in ("kyotocabinet", "tkrzw"):
                with self.subTest(backend=backend):
                    ordered, arguments, _ = self.relink(work, environment, work / "policy", features=backend)
                    self.assertEqual(ordered.returncode, 0, ordered.stderr)
                    self.assertIn(b"--section-ordering-file", arguments)
                    self.assertIn("using measured policy", ordered.stderr)

    def test_default_policy_for_each_backend_is_strict_and_foreign_named_policy_fails(self):
        with scratch() as work:
            environment = self.prepare(work)
            environment.pop('OXPINYIN_FUNCTION_ORDER_POLICY')
            scripts = work / 'tools/packaging'
            scripts.mkdir(parents=True)
            for feature, backend in (('bdb', 'bdb'), ('kyotocabinet', 'kc'), ('tkrzw', 'tkrzw')):
                policy = scripts / 'function-order' / backend / 'libpinyin.order'
                policy.parent.mkdir(parents=True)
                shutil.copyfile(work / 'policy', policy)
                ordered, arguments, _ = self.relink(work, environment,
                    script=scripts / 'relink-versioned.sh', features=feature)
                self.assertEqual(ordered.returncode, 0, ordered.stderr)
                self.assertIn(b'--section-ordering-file', arguments)
                policy.write_text(HEADER + 'init\tstale\n')
                stale, arguments, _ = self.relink(work, environment,
                    script=scripts / 'relink-versioned.sh', features=feature)
                self.assertEqual(stale.returncode, 1, stale.stderr)
                self.assertIsNone(arguments)
                self.assertIn('unresolved policy path: stale', stale.stderr)
                policy.unlink()
                missing, arguments, _ = self.relink(work, environment,
                    script=scripts / 'relink-versioned.sh', features=feature)
                self.assertEqual(missing.returncode, 1, missing.stderr)
                self.assertIsNone(arguments)
                self.assertIn('policy not found', missing.stderr)
                shutil.copyfile(work / 'policy', policy)
            foreign = scripts / 'function-order/bdb/libpinyin.order'
            alias = work / 'alias.order'
            alias.symlink_to(foreign)
            for policy in (foreign, alias):
                refused, arguments, _ = self.relink(work, environment, policy,
                    script=scripts / 'relink-versioned.sh', features='kyotocabinet')
                self.assertEqual(refused.returncode, 1, refused.stderr)
                self.assertIsNone(arguments)
                self.assertIn('policy backend bdb does not match kc', refused.stderr)

    def test_install_environment_opt_in_uses_the_facade_policy(self):
        with scratch() as work:
            environment = self.prepare(work)
            result, arguments, _ = self.relink(
                work, {**environment, "OXPINYIN_FUNCTION_ORDER_POLICY": str(work / "policy")})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(b"--section-ordering-file", arguments)

    def test_opt_in_adds_only_the_order_argument(self):
        with scratch() as work:
            environment = self.prepare(work)
            _, base_args, _ = self.relink(work, environment)
            ordered, arguments, _ = self.relink(work, environment, work / "policy")
            self.assertEqual(ordered.returncode, 0, ordered.stderr)
            index = arguments.index(b"--section-ordering-file")
            self.assertEqual(arguments[index - 1:index + 2],
                             [b"-Xlinker", b"--section-ordering-file", b"-Xlinker"])
            self.assertTrue(arguments[index + 2].endswith(b"/order.ld"))
            self.assertEqual(arguments[:index - 1] + arguments[index + 3:], base_args)

    def test_every_link_route_carries_z_defs(self):
        with scratch() as work:
            environment = self.prepare(work)
            unordered, base_args, _ = self.relink(work, environment)
            ordered, arguments, _ = self.relink(work, environment, work / "policy")
            fallback, fallback_args, _ = self.relink(
                work, {**environment, "TEST_LINKER_VERSION": "LLD 20"}, work / "policy")
            for result, args in ((unordered, base_args), (ordered, arguments), (fallback, fallback_args)):
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(args.count(b"-Wl,-z,defs"), 1)

    def test_link_and_export_failures_do_not_install_or_retry(self):
        with scratch() as work:
            environment = self.prepare(work)
            for failure in ({"TEST_LINK_EXIT": "1"}, {"TEST_EXTRA_EXPORT": "1"}):
                result, arguments, data = self.relink(work, {**environment, **failure}, work / "policy")
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertEqual(data, b"original installed bytes")
                self.assertIn(b"--section-ordering-file", arguments)


# Mach-O: llvm-nm's darwin format carries the section and raw name, its
# demangled BSD format the identity; the order file names raw symbols.
MACHO_DARWIN = """
pinyin.pinyin.11abf23f73cf6425-cgu.0.rcgu.o:
0000000000000100 (__TEXT,__text) external _pinyin_init
0000000000000120 (__TEXT,__text) non-external __RNvCs1w45a2lSNJp_6pinyin4open
0000000000000140 (__TEXT,__text) non-external [cold func] __RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin
0000000000000160 (__TEXT,__text) non-external (was a private external) ___udivti3
0000000000000180 (__TEXT,__text) private external __ZN12kyotocabinet6HashDB3getEv
00000000000001a0 (__TEXT,__text) non-external __RNvCsBBBB_5crate4keep
0000000000000010 (__TEXT,__gcc_except_tab) non-external GCC_except_table0
0000000000000020 (__TEXT,__const) non-external __RNvCsBBBB_5crate5TABLE
                 (undefined) external _write (from libSystem)
"""
MACHO_BSD = """
pinyin.pinyin.11abf23f73cf6425-cgu.0.rcgu.o:
0000000000000100 T _pinyin_init
0000000000000120 t pinyin::open
0000000000000140 t oxpinyin_core::insert::<&str>
0000000000000160 t ___udivti3
0000000000000180 t kyotocabinet::HashDB::get()
00000000000001a0 t crate::keep
0000000000000010 t GCC_except_table0
0000000000000020 t crate::TABLE
"""
MACHO_NAMES = ['pinyin_init', 'oxpinyin_core::insert::<&str> [instantiating crate: pinyin]', '__udivti3',
               '_ZN12kyotocabinet6HashDB3getEv', 'pinyin::open', 'std::sys::random::linux::hashmap_random_keys']
MACHO_ORDERED = ['_pinyin_init', '__RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin', '___udivti3',
                 '__ZN12kyotocabinet6HashDB3getEv', '__RNvCs1w45a2lSNJp_6pinyin4open']
OTOOL = "Load command 1\n      cmd LC_SEGMENT_64\nSection\n  sectname __text\n   segname __TEXT\n      addr 0x%016x\n      size 0x0000000000000100\nSection\n  sectname __const\n   segname __TEXT\n      addr 0x0000000000001000\n"


def darwin_records(entries):
    return ''.join(f'{address:016x} (__TEXT,__text) non-external {name}\n' for name, address in entries)


def fake_run(darwin, otool=OTOOL % 0x100, policy_output=None, status=0):
    """A captured-process adapter answering nm, otool and the AWK policy reader."""
    def run(command, **_):
        if command[0] == 'awk':
            return pool.Completed(status, (policy_output or '').encode(), b'bad policy\n' if status else b'')
        if command[0] == 'otool':
            return pool.Completed(0, otool.encode(), b'')
        self_text = MACHO_BSD if '--demangle' in command else darwin
        return pool.Completed(0, self_text.encode(), b'')
    return run


class MachoTests(unittest.TestCase):
    def test_identity_removes_the_mach_o_underscore_and_decodes_the_instantiating_crate(self):
        identity = ORDER['macho_identity']
        self.assertEqual(identity('_pinyin_init', '_pinyin_init'), 'pinyin_init')
        self.assertEqual(identity('___udivti3', '___udivti3'), '__udivti3')
        self.assertEqual(identity('__RNvCs1w45a2lSNJp_6pinyin4open', 'pinyin::open'), 'pinyin::open')
        self.assertEqual(identity('__RINvCsAAAA_13oxpinyin_core6insertReECs1w45a2lSNJp_6pinyin', 'oxpinyin_core::insert::<&str>'),
                         'oxpinyin_core::insert::<&str> [instantiating crate: pinyin]')
        self.assertEqual(identity('__ZN12kyotocabinet6HashDB3getEv', 'kyotocabinet::HashDB::get()'),
                         '_ZN12kyotocabinet6HashDB3getEv')
        self.assertIsNone(identity('__RNvCsBBBB_5crate4keep', '__RNvCsBBBB_5crate4keep'))
        self.assertIsNone(identity('__ZN5crate4keep17h0123456789abcdefE', '__ZN5crate4keep17h0123456789abcdefE'))
        self.assertEqual(identity('_crate_c_function.llvm.1234', '_crate_c_function.llvm.1234'), 'crate_c_function')
        self.assertEqual(identity('__RNvCs1w45a2lSNJp_6pinyin4open.1482', 'pinyin::open (.1482)'), 'pinyin::open')

    def test_symbols_join_both_nm_views_and_reject_disagreement(self):
        symbols = ORDER['macho_symbols'](MACHO_DARWIN, MACHO_BSD)
        self.assertEqual([(identity, section) for identity, _, _, section in symbols], [
            ('pinyin_init', '__TEXT,__text'), ('pinyin::open', '__TEXT,__text'),
            ('oxpinyin_core::insert::<&str> [instantiating crate: pinyin]', '__TEXT,__text'),
            ('__udivti3', '__TEXT,__text'), ('_ZN12kyotocabinet6HashDB3getEv', '__TEXT,__text'),
            ('crate::keep', '__TEXT,__text'), ('GCC_except_table0', '__TEXT,__gcc_except_tab'),
            ('crate::TABLE', '__TEXT,__const')])
        self.assertEqual([address for _, _, address, _ in symbols][:2], [0x100, 0x120])
        with self.assertRaisesRegex(ValueError, 'lengths differ'):
            ORDER['macho_symbols'](MACHO_DARWIN, MACHO_BSD + '0000000000000200 t extra\n')
        with self.assertRaisesRegex(ValueError, 'records disagree'):
            ORDER['macho_symbols'](MACHO_DARWIN, MACHO_BSD.replace('0000000000000120', '0000000000000124'))

    def test_order_resolves_in_policy_order_reports_unresolved_and_refuses_ambiguity(self):
        symbols = ORDER['macho_symbols'](MACHO_DARWIN, MACHO_BSD)
        ordered, unresolved = ORDER['macho_order'](MACHO_NAMES, symbols)
        self.assertEqual(ordered, MACHO_ORDERED)
        self.assertEqual(unresolved, ['std::sys::random::linux::hashmap_random_keys'])
        # Only __TEXT,__text bodies resolve; a repeated policy name maps to one body once.
        self.assertEqual(ORDER['macho_order'](['crate::TABLE', 'pinyin_init', 'pinyin_init'], symbols),
                         (['_pinyin_init'], ['crate::TABLE']))
        twins = ORDER['macho_symbols'](
            MACHO_DARWIN + '00000000000001c0 (__TEXT,__text) non-external __RNvCsCCCC_5crate4keep\n',
            MACHO_BSD + '00000000000001c0 t crate::keep\n')
        with self.assertRaisesRegex(ValueError, 'ambiguous policy path: crate::keep'):
            ORDER['macho_order'](['crate::keep'], twins)
        # Every body of one clone family is ordered, in symbol-table order.
        clones = ORDER['macho_symbols'](
            MACHO_DARWIN + '00000000000001c0 (__TEXT,__text) non-external __RNvCsBBBB_5crate4keep.1482\n',
            MACHO_BSD + '00000000000001c0 t crate::keep (.1482)\n')
        self.assertEqual(ORDER['macho_order'](['crate::keep'], clones),
                         (['__RNvCsBBBB_5crate4keep', '__RNvCsBBBB_5crate4keep.1482'], []))

    def test_order_file_reuses_the_policy_validation_and_needs_one_resolved_path(self):
        run = fake_run(MACHO_DARWIN, policy_output='\n'.join(MACHO_NAMES) + '\n')
        order, names, ordered, unresolved = ORDER['macho_order_file'](Path('/p/libpinyin.order'), '/a/libpinyin.a',
                                                                     'libpinyin.so.15', run=run)
        self.assertEqual((names, ordered), (MACHO_NAMES, MACHO_ORDERED))
        self.assertEqual(unresolved, ['std::sys::random::linux::hashmap_random_keys'])
        self.assertEqual(order.decode().splitlines(),
                         ['# oxpinyin function order: libpinyin.order (libpinyin.so.15) resolved against libpinyin.a',
                          *MACHO_ORDERED])
        with self.assertRaisesRegex(ValueError, 'awk: bad policy'):
            ORDER['macho_order_file'](Path('/p/libpinyin.order'), '/a/libpinyin.a', 'libpinyin.so.15',
                                      run=fake_run(MACHO_DARWIN, policy_output='x\n', status=1))
        with self.assertRaisesRegex(ValueError, 'no policy path resolves'):
            ORDER['macho_order_file'](Path('/p/libpinyin.order'), '/a/libpinyin.a', 'libpinyin.so.15',
                                      run=fake_run(MACHO_DARWIN, policy_output='absent\n'))

    def test_text_start_requires_one_text_section(self):
        self.assertEqual(ORDER['macho_text_start'](OTOOL % 0x2570), 0x2570)
        for text in ('', OTOOL % 0x100 + OTOOL % 0x200, OTOOL.replace('segname __TEXT', 'segname __OTHER') % 0x100):
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, 'one Mach-O __TEXT,__text'):
                ORDER['macho_text_start'](text)

    def check(self, entries, ordered=('a', 'b'), start=0x100):
        return ORDER['verify_macho'](Path('/synthetic/libpinyin.dylib'), list(ordered),
                                     run=fake_run(darwin_records(entries), OTOOL % start))

    def test_layout_prefix_passes_fails_and_reports_dead_stripped_entries(self):
        layout, stripped = self.check([('a', 0x100), ('b', 0x120), ('crt', 0x140)])
        self.assertEqual((layout['paths'], layout['text_start'], stripped), (2, '0x100', []))
        layout, stripped = self.check([('a', 0x100), ('b', 0x100)])
        self.assertEqual((layout['paths'], stripped), (2, []))
        layout, stripped = self.check([('a', 0x100), ('c', 0x120), ('crt', 0x140)], ordered=('a', 'b', 'c'))
        self.assertEqual((layout['paths'], stripped), (2, ['b']))
        with self.assertRaisesRegex(ValueError, 'no ordered function'):
            self.check([('crt', 0x100)])
        with self.assertRaisesRegex(ValueError, 'start of __TEXT,__text'):
            self.check([('crt', 0x100), ('a', 0x120), ('b', 0x140)])
        with self.assertRaisesRegex(ValueError, 'start of __TEXT,__text'):
            self.check([('a', 0x120), ('b', 0x140)])
        with self.assertRaises(ValueError):
            self.check([('b', 0x100), ('a', 0x120)])
        with self.assertRaisesRegex(ValueError, 'out of order'):
            self.check([('a', 0x100), ('c', 0x120), ('b', 0x140)], ordered=('a', 'b', 'c'))
        with self.assertRaisesRegex(ValueError, 'interrupts policy prefix'):
            self.check([('a', 0x100), ('crt', 0x120), ('b', 0x140)])
        with self.assertRaisesRegex(ValueError, 'outside the prefix'):
            self.check([('a', 0x100), ('b', 0x120), ('crt', 0x140), ('b', 0x160)])
        # Symbols outside __TEXT,__text neither order nor interrupt.
        run = fake_run(darwin_records([('a', 0x100), ('b', 0x120)]) +
                       '0000000000000110 (__TEXT,__const) non-external table\n')
        layout, _ = ORDER['verify_macho'](Path('/synthetic/libpinyin.dylib'), ['a', 'b'], run=run)
        self.assertEqual(layout['paths'], 2)


class MachoRelinkTests(unittest.TestCase):
    def prepare(self, work):
        scripts = {
            "uname": "#!/bin/sh\nprintf '%s\\n' \"${TEST_UNAME:-Darwin}\"\n",
            "rustc": "#!/bin/sh\nprintf 'MACOSX_DEPLOYMENT_TARGET=11.0\\n'\n",
            "compiler": r"""#!/bin/sh
printf '%s\000' "$@" > "$TEST_LINK_ARGS"
out=''
previous=''
for arg; do
  if [ "$previous" = '-o' ]; then out=$arg; fi
  case "$arg" in
    -Wl,-order_file,*) cp "${arg#-Wl,-order_file,}" "$TEST_ORDER_COPY";;
    -Wl,-exported_symbols_list,*) cp "${arg#-Wl,-exported_symbols_list,}" "$TEST_EXPORTS_COPY";;
  esac
  previous=$arg
done
printf 'synthetic linked bytes\n' > "$out"
exit "${TEST_LINK_EXIT:-0}"
""",
            "nm": r"""#!/bin/sh
for arg; do last=$arg; done
case " $* " in
  *" --format=darwin "*) case "$last" in *.a) cat "$TEST_NM_DARWIN";; *) cat "$TEST_NM_LINKED";; esac;;
  *" -g "*) cat "$TEST_NM_EXPORTS";;
  *) cat "$TEST_NM_BSD";;
esac
""",
            "otool": "#!/bin/sh\ncat \"$TEST_OTOOL\"\n",
        }
        for name, contents in scripts.items():
            (work / name).write_text(contents)
            (work / name).chmod(0o755)
        (work / 'darwin').write_text(MACHO_DARWIN)
        (work / 'linked').write_text(MACHO_DARWIN)
        (work / 'bsd').write_text(MACHO_BSD)
        (work / 'exports').write_text('0000000000000100 T _pinyin_init\n')
        (work / 'otool.txt').write_text(OTOOL % 0x100)
        (work / 'policy').write_text(HEADER + 'init\tpinyin_init\ninit\tpinyin::open\nsteady\tabsent::path\n')
        (work / 'version').write_text('LIBPINYIN {\n global:\n pinyin_init;\n local:\n *;\n};\n')
        (work / 'library.a').write_bytes(b'synthetic archive')
        (work / 'no-pc').mkdir()
        environment = {**os.environ, 'PATH': str(work) + os.pathsep + os.environ['PATH'],
                       'TEST_LINK_ARGS': str(work / 'args'), 'TEST_ORDER_COPY': str(work / 'order-copy'),
                       'TEST_EXPORTS_COPY': str(work / 'exports-copy'), 'TEST_NM_DARWIN': str(work / 'darwin'),
                       'TEST_NM_LINKED': str(work / 'linked'), 'TEST_NM_BSD': str(work / 'bsd'),
                       'TEST_NM_EXPORTS': str(work / 'exports'), 'TEST_OTOOL': str(work / 'otool.txt'),
                       'PKG_CONFIG_LIBDIR': str(work / 'no-pc'), 'OXPINYIN_FUNCTION_ORDER_POLICY': ''}
        environment.pop('OXPINYIN_BDB_LIB_DIR', None)
        environment.pop('CC', None)
        return environment

    def relink(self, work, environment, policy=None, root=None, features=None):
        destination = work / 'libpinyin.15.0.0.dylib'
        destination.write_bytes(b'original installed bytes')
        driver = 'import sys\nfrom pathlib import Path\nfrom tools.dev import packaging\nclass FixtureTools(packaging.Tools):\n def heavy(self, argv, **options): self.run(argv, **options)\nsys.exit(packaging.main(sys.argv[2:], tools=FixtureTools(root=Path(sys.argv[1]))))'
        command = [sys.executable, '-X', 'lazy_imports=normal', '-c', driver, str(root or ROOT), 'relink-macho',
                   '--staticlib', str(work / 'library.a'), '--ver', str(work / 'version'),
                   '--soname', 'libpinyin.so.15', '--dest', str(destination),
                   '--install-name', '/opt/stage/lib/libpinyin.15.dylib', '--cc', str(work / 'compiler')]
        if policy is not None:
            command += ['--function-order', str(policy)]
        if features is not None:
            command += ['--features', features]
        for name in ('args', 'order-copy', 'exports-copy'):
            (work / name).unlink(missing_ok=True)
        result = subprocess.run(command, capture_output=True, text=True, check=False, env=environment)
        arguments = None
        if (work / 'args').exists():
            arguments = (work / 'args').read_bytes().split(b'\0')[:-1]
            arguments[arguments.index(b'-o') + 1] = b'OUTPUT'
        self.assertEqual(list(work.glob('*.relink.*')), [], 'relink scratch leaked')
        return result, arguments, destination.read_bytes()

    def test_opt_out_keeps_the_original_object_without_linking(self):
        with scratch() as work:
            environment = self.prepare(work)
            result, arguments, data = self.relink(work, environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIsNone(arguments)
            self.assertEqual(data, b'original installed bytes')
            self.assertIn('disabled by explicit opt-out', result.stderr)
            self.assertNotIn('relinked', result.stdout)

    def test_ordered_relink_passes_the_order_file_and_reports_resolution(self):
        with scratch() as work:
            environment = self.prepare(work)
            result, arguments, data = self.relink(work, environment, work / 'policy')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(data, b'synthetic linked bytes\n')
            order = [word for word in arguments if word.startswith(b'-Wl,-order_file,')]
            self.assertEqual(len(order), 1)
            self.assertTrue(order[0].endswith(b'/order.txt'))
            self.assertEqual((work / 'order-copy').read_text().splitlines()[1:],
                             ['_pinyin_init', '__RNvCs1w45a2lSNJp_6pinyin4open'])
            self.assertTrue((work / 'order-copy').read_text().startswith('# oxpinyin function order: policy'))
            self.assertEqual((work / 'exports-copy').read_text(), '_pinyin_init\n')
            for word in (b'-dynamiclib', b'-mmacosx-version-min=11.0', b'-Wl,-S', b'-Wl,-dead_strip',
                         b'-Wl,-install_name,/opt/stage/lib/libpinyin.15.dylib', b'-Wl,-compatibility_version,15',
                         b'-Wl,-current_version,15.0.0', b'-Wl,-headerpad_max_install_names',
                         b'-Wl,-u,_pinyin_init', b'-ldb', b'-liconv'):
                self.assertIn(word, arguments)
            # The archive loads on demand behind the export roots, before the libraries.
            archive = arguments.index(os.fsencode(str(work / 'library.a')))
            self.assertLess(arguments.index(b'-Wl,-u,_pinyin_init'), archive)
            self.assertLess(archive, arguments.index(b'-ldb'))
            self.assertNotIn(b'-Xlinker', arguments)
            self.assertFalse([word for word in arguments if word.startswith(b'-Wl,-force_load')])
            self.assertIn('2 of 3 policy paths resolve on Mach-O (2 symbols ordered); 1 unresolved', result.stderr)
            self.assertIn('unresolved policy path on Mach-O: absent::path', result.stderr)
            self.assertIn('using measured policy', result.stderr)
            self.assertIn('2 ordered symbols form the __TEXT,__text prefix at 0x100; 0 dead-stripped', result.stderr)
            self.assertIn('relinked ' + str(work / 'libpinyin.15.0.0.dylib'), result.stdout)
            self.assertIn('exports: 1 (LIBPINYIN)', result.stdout)

    def test_backend_library_directory_and_named_compiler_reach_the_link(self):
        with scratch() as work:
            environment = {**self.prepare(work), 'OXPINYIN_BDB_LIB_DIR': '/opt/keg/lib'}
            result, arguments, _ = self.relink(work, environment, work / 'policy')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertLess(arguments.index(b'-L/opt/keg/lib'), arguments.index(b'-ldb'))
            result, arguments, _ = self.relink(work, environment, work / 'policy', features='tkrzw')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(b'-ltkrzw', arguments)
            self.assertNotIn(b'-L/opt/keg/lib', arguments)

    def test_checked_in_policy_follows_the_backend_and_foreign_policies_are_refused(self):
        with scratch() as work:
            environment = self.prepare(work)
            environment.pop('OXPINYIN_FUNCTION_ORDER_POLICY')
            scripts = work / 'tools/packaging'
            for backend in ('bdb', 'kc', 'tkrzw'):
                policy = scripts / 'function-order' / backend / 'libpinyin.order'
                policy.parent.mkdir(parents=True)
                shutil.copyfile(work / 'policy', policy)
            for feature in ('bdb', 'kyotocabinet', 'tkrzw'):
                with self.subTest(feature=feature):
                    result, arguments, data = self.relink(work, environment, root=work, features=feature)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn('using measured policy ' + str(scripts / 'function-order'), result.stderr)
                    self.assertEqual(data, b'synthetic linked bytes\n')
            foreign = scripts / 'function-order/bdb/libpinyin.order'
            result, arguments, data = self.relink(work, environment, foreign, root=work, features='kyotocabinet')
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertEqual(data, b'original installed bytes')
            self.assertIn('policy backend bdb does not match kc', result.stderr)
            (scripts / 'function-order/tkrzw/libpinyin.order').unlink()
            result, arguments, data = self.relink(work, environment, root=work, features='tkrzw')
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertIn('policy not found', result.stderr)

    def test_dead_stripped_ordered_paths_are_reported_not_refused(self):
        with scratch() as work:
            environment = self.prepare(work)
            (work / 'linked').write_text(darwin_records([('_pinyin_init', 0x100), ('_crt', 0x120)]))
            result, _, data = self.relink(work, environment, work / 'policy')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(data, b'synthetic linked bytes\n')
            self.assertIn('dead-stripped): __RNvCs1w45a2lSNJp_6pinyin4open', result.stderr)
            self.assertIn('1 ordered symbols form the __TEXT,__text prefix at 0x100; 1 dead-stripped', result.stderr)

    def test_layout_export_and_link_failures_do_not_install(self):
        with scratch() as work:
            environment = self.prepare(work)
            cases = {
                'unordered layout': lambda: (work / 'linked').write_text(
                    darwin_records([('_crt', 0x100), ('_pinyin_init', 0x120), ('__RNvCs1w45a2lSNJp_6pinyin4open', 0x140)])),
                'interleaved layout': lambda: (work / 'linked').write_text(
                    darwin_records([('_pinyin_init', 0x100), ('_crt', 0x120), ('__RNvCs1w45a2lSNJp_6pinyin4open', 0x140)])),
                'extra export': lambda: (work / 'exports').write_text('0000000000000100 T _pinyin_init\n0000000000000200 T _leak\n'),
            }
            for case, arrange in cases.items():
                with self.subTest(case=case):
                    arrange()
                    result, arguments, data = self.relink(work, environment, work / 'policy')
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIsNotNone(arguments)
                    self.assertEqual(data, b'original installed bytes')
                    self.assertIn('layout check failed' if 'layout' in case else 'export set differs', result.stderr)
                    (work / 'linked').write_text(MACHO_DARWIN)
                    (work / 'exports').write_text('0000000000000100 T _pinyin_init\n')
            result, arguments, data = self.relink(work, {**environment, 'TEST_LINK_EXIT': '1'}, work / 'policy')
            self.assertNotEqual(result.returncode, 0)
            self.assertIsNotNone(arguments)
            self.assertEqual(data, b'original installed bytes')

    def test_unresolvable_policy_fails_before_the_link(self):
        with scratch() as work:
            environment = self.prepare(work)
            (work / 'policy').write_text(HEADER + 'init\tabsent\n')
            result, arguments, data = self.relink(work, environment, work / 'policy')
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertEqual(data, b'original installed bytes')
            self.assertIn('no policy path resolves against the Mach-O archive', result.stderr)
            (work / 'policy').write_text(HEADER + 'init\tabsent\ninit\tabsent\n')
            result, arguments, _ = self.relink(work, environment, work / 'policy')
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertIn('invalid or duplicate policy row', result.stderr)

    def test_other_hosts_are_refused(self):
        with scratch() as work:
            environment = {**self.prepare(work), 'TEST_UNAME': 'Linux'}
            result, arguments, data = self.relink(work, environment, work / 'policy')
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIsNone(arguments)
            self.assertEqual(data, b'original installed bytes')
            self.assertIn('macOS-only', result.stderr)


if __name__ == "__main__":
    unittest.main()
