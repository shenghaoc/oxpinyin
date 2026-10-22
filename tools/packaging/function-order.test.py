#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Source regressions for the optional shipping link ordering; no compiler or
profiler is needed.

Synthetic nm records exercise capability probing, policy resolution, fallback
transport and cleanup. Real-library placement, reproducible builds, fallback
byte identity and performance still need the shipping gate.
"""

from contextlib import contextmanager
import argparse
import io
import json
import os
from pathlib import Path
import shutil
import runpy
import signal
import subprocess
import tempfile
import unittest
from unittest import mock


HERE = Path(__file__).resolve().parent
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
            command = ["bash", str(HERE / "function-order.sh"), mode, "unused.a"]
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

    def test_recognized_clone_family_includes_every_body(self):
        name = "alloc::boxed::box_new_uninit"
        base = ".text._RNvFake14box_new_uninit"
        result = self.invoke("resolve", row(name, base) + row(name, base + ".1923"),
                             HEADER + "init\t" + name + "\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'*("{base}")', result.stdout)
        self.assertIn(f'*("{base}.1923")', result.stdout)
        self.assertEqual(result.stdout.count('*("'), 2)


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
            command = ['bash', '-c', 'source "$1"; function_order_supported "$2" unused.a "$3"',
                       'capability-test', str(HERE / "function-order.sh"), str(compiler), str(work)]
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
            command = ['bash', str(HERE / 'function-order.sh'), 'policy', str(policy), 'libpinyin.so.15']
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
            return ORDER['verify'](LIBRARY, Path('/policy'), 'pinyin')

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

    def test_two_unrelated_bodies_cannot_masquerade_as_clones(self):
        with self.assertRaisesRegex(ValueError, 'ambiguous policy path'):
            self.check([('a', 0x100), ('a', 0x110), ('b', 0x120)],
                       raw=[('body_one', 0x100), ('body_two', 0x110), ('b', 0x120)])

    def test_each_arm_is_checked_and_result_is_reported(self):
        arms = {arm: {facade: LIBRARY for facade in ('pinyin', 'zhuyin')}
                for arm in ('base', 'pr', 'control', 'pin')}
        args = argparse.Namespace(function_order=['base/pinyin=/old-policy'], unordered_arm=[])
        with mock.patch.dict(ORDER['check_arms'].__globals__, verify=mock.Mock(return_value={}),
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
                             linker_description=lambda *_: 'GNU ld 2.47'), \
                mock.patch('sys.stdout', new_callable=io.StringIO) as output:
            with self.assertRaisesRegex(ValueError, 'arm pr/pinyin refused.*GNU ld 2.47'):
                ORDER['check_arms'](argparse.Namespace(), {'pr': {'pinyin': LIBRARY}})
        self.assertEqual(json.loads(output.getvalue())['function_order'], 'refused')

    def test_one_refusal_still_reports_the_other_arms_before_stopping(self):
        arms = {arm: {'pinyin': LIBRARY} for arm in ('base', 'pr', 'control', 'pin')}
        verify = mock.Mock(side_effect=[ValueError('unordered'), {}, ValueError('fallback')])
        with mock.patch.dict(ORDER['check_arms'].__globals__, verify=verify,
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


class MeasurementCallerTests(unittest.TestCase):
    def test_input_library_tools_refuse_before_creating_output_or_starting_a_worker(self):
        with scratch() as work:
            library = work / 'unordered.so'
            library.write_bytes(b'not an ordered ELF')
            scripts = [('run-rss-diag.sh', 'S', ['--lp', str(library), '--ox', str(library),
                                               '--data', str(work), '--out', str(work / 'output')]),
                       ('run-system-import-profile.sh', 'ox', [str(library), str(library), str(work),
                                                              str(work / 'output'), '--cold-open'])]
            for script, arm, arguments in scripts:
                with self.subTest(script=script):
                    result = subprocess.run(['bash', str(HERE.parent / 'bisection' / script), *arguments],
                                            capture_output=True, text=True)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f'arm {arm}/pinyin refused', result.stderr)
                    self.assertFalse((work / 'output').exists())

    def test_snapshot_checks_before_stripping_or_building_the_fixture(self):
        with scratch() as work:
            script = HERE.parent / 'perf-gate/snapshot.sh'
            command = r'''
# Satisfy snapshot's tool-presence preflight on hosts without GNU binutils.
# The invalid ELF must be refused before any build/strip command can run.
cargo() { echo 'unexpected cargo' >&2; exit 99; }
gcc() { echo 'unexpected gcc' >&2; exit 99; }
readelf() { echo 'unexpected shell readelf' >&2; exit 99; }
nm() { echo 'unexpected shell nm' >&2; exit 99; }
tools/packaging/install.sh() {
  for arg; do case "$arg" in --destdir=*) stage=${arg#*=};; esac; done
  mkdir -p "$stage/usr/lib"
  printf 'not an ordered ELF' > "$stage/usr/lib/libpinyin.so.15.0.0"
}
strip() { echo 'unexpected strip' >&2; exit 99; }
source "$0"
'''
            result = subprocess.run(['bash', '-c', command, str(script), '--backend', 'kyotocabinet',
                                     '--out', str(work / 'snapshot.json')], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('arm shipped/pinyin refused', result.stderr)
            self.assertNotIn('unexpected strip', result.stderr)
            self.assertNotIn('building the alloc-count', result.stdout)
            self.assertFalse((work / 'snapshot.json').exists())

    def test_legacy_runner_guards_pass_every_measured_arm_to_the_shared_check(self):
        expected = {'run-perf-baseline.sh': ('ox', 'pin'), 'run-perf-matrix.sh': ('C', 'D', 'A', 'B'),
                    'run-perf-same-data.sh': ('C', 'D', 'A', 'B'),
                    'run-shipped-check.sh': ('Z', 'S', 'L'),
                    'run-tree-recipe-control.sh': ('X', 'Y', 'Z', 'S', 'L')}
        for script, arms in expected.items():
            text = (HERE.parent / 'bisection' / script).read_text()
            start = text.index('function_order_measurement ')
            guard = text[start:].split('\n\n', 1)[0]
            environment = {**os.environ}
            for variable in ('CAPI_SO', 'ORACLE_SO', 'A_SO', 'B_SO', 'C_SO', 'D_SO',
                             'L_SO', 'X_SO', 'Y_SO', 'Z_SO', 'S_SO'):
                environment[variable] = str(LIBRARY)
            environment.update(OXPINYIN_TKRZW_SO=str(LIBRARY), OXPINYIN_KC_SO=str(LIBRARY))
            shell = 'source "$1"; FUNCTION_ORDER_ARGS=(); function_order_measurement() { printf "%s\\n" "$@"; }; ' + guard
            result = subprocess.run(['bash', '-c', shell, 'guard-test', str(HERE / 'function-order.sh')],
                                    capture_output=True, text=True, env=environment)
            with self.subTest(script=script):
                self.assertEqual(result.returncode, 0, result.stderr)
                for arm in arms:
                    self.assertIn(f'{arm}/pinyin={LIBRARY}', result.stdout)
                self.assertLess(start, text.index('SPEED_JSONL=' if script in ('run-perf-baseline.sh', 'run-perf-matrix.sh', 'run-perf-same-data.sh')
                                                 else 'for n in $SIZES'))


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

    def relink(self, work, environment, policy=None, script=None):
        destination = work / "libpinyin.so.15.0.0"
        destination.write_bytes(b"original installed bytes")
        command = ["bash", str(script or HERE / "relink-versioned.sh"), "--staticlib", str(work / "library.a"),
                   "--ver", str(work / "version"), "--soname", "libpinyin.so.15", "--dest", str(destination),
                   "--cc", str(work / "compiler")]
        if policy is not None:
            command += ["--function-order", str(policy)]
        result = subprocess.run(command, capture_output=True, text=True, check=False, env=environment)
        arguments = (work / "args").read_bytes().split(b"\0")[:-1]
        arguments[arguments.index(b"-o") + 1] = b"OUTPUT"
        self.assertEqual(list(work.glob("*.relink.*")), [], "relink scratch leaked")
        return result, arguments, destination.read_bytes()

    def test_explicit_empty_skips_query_and_fallback_preserves_link_arguments(self):
        with scratch() as work:
            environment = self.prepare(work)
            baseline, base_args, base_bytes = self.relink(work, environment)
            self.assertEqual(baseline.returncode, 0, baseline.stderr)
            self.assertFalse((work / "queries").exists())
            for version in ("GNU gold (GNU Binutils) 1.16", "LLD 20"):
                fallback, arguments, data = self.relink(
                    work, {**environment, "TEST_LINKER_VERSION": version}, work / "policy")
                self.assertEqual(fallback.returncode, 0, fallback.stderr)
                self.assertEqual(arguments, base_args)
                self.assertEqual(data, base_bytes)
                self.assertIn("original layout", fallback.stderr)
            fallback, arguments, data = self.relink(
                work, {**environment, "TEST_LINKER_HELP": "no ordering option"}, work / "policy")
            self.assertEqual(fallback.returncode, 0, fallback.stderr)
            self.assertEqual(arguments, base_args)
            self.assertEqual(data, base_bytes)

    def test_default_selects_its_soname_policy_and_unsupported_linker_falls_back(self):
        with scratch() as work:
            environment = self.prepare(work)
            _, base_args, base_bytes = self.relink(work, environment)
            scripts = work / "packaging"
            (scripts / "function-order").mkdir(parents=True)
            for filename in ("relink-versioned.sh", "function-order.sh", "function-order.awk"):
                shutil.copyfile(HERE / filename, scripts / filename)
            shutil.copyfile(work / "policy", scripts / "function-order/libpinyin.order")
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

    def test_inapplicable_policy_keeps_original_arguments(self):
        with scratch() as work:
            environment = self.prepare(work)
            _, base_args, base_bytes = self.relink(work, environment)
            (work / "policy").write_text(HEADER + "init\tmissing\n")
            fallback, arguments, data = self.relink(work, environment, work / "policy")
            self.assertEqual(fallback.returncode, 0, fallback.stderr)
            self.assertEqual(arguments, base_args)
            self.assertEqual(data, base_bytes)

    def test_unavailable_demangler_keeps_original_arguments(self):
        with scratch() as work:
            environment = self.prepare(work)
            _, base_args, base_bytes = self.relink(work, environment)
            fallback, arguments, data = self.relink(
                work, {**environment, "TEST_NM_EXIT": "1"}, work / "policy")
            self.assertEqual(fallback.returncode, 0, fallback.stderr)
            self.assertEqual(arguments, base_args)
            self.assertEqual(data, base_bytes)

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


if __name__ == "__main__":
    unittest.main()
