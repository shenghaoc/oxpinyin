#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Source regressions for optional ordering; no compiler or profiler needed.

Run only after the scheduling hold is released. Synthetic nm/profiler records
exercise parsing and fallback transport; real-library placement, reproducible
builds, fallback byte identity and performance still need the shipping gate.
"""

from contextlib import contextmanager
import io
import os
from pathlib import Path
import runpy
import signal
import subprocess
import tempfile
import unittest
from unittest import mock


HERE = Path(__file__).resolve().parent
GENERATOR = runpy.run_path(str(HERE.parent / "generate-function-order.py"))
LIBRARY = Path("/synthetic/libpinyin.so")
HEADER = "oxpinyin-function-order-v1\tlibpinyin.so.15\n"


def row(name, section, address="0000000000000000"):
    return f"{name}|{address}| t | FUNC |0000000000000010| |{section}\n"


def profile(body, total):
    return f"version: 1\npositions: instr line\nevents: Ir\n{body}totals: {total}\n"


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


class GeneratorTests(unittest.TestCase):
    def parse(self, text):
        return GENERATOR["callgrind_functions"](io.StringIO(text), LIBRARY)

    def test_exclusive_costs_compressed_names_and_exact_object(self):
        text = profile(
            "ob=(1) /synthetic/libpinyin.so\nfn=(1) ancestor\ncfn=(2) callee\n"
            "calls=1 0x10 1\n0x1 1 90\n"
            "fn=(2)\n0x10 1 7\n+1 * 2\n"
            "ob=(2) /synthetic/libpinyin.so.extra\nfn=(3) foreign\n0x20 2 4\n"
            "ob=(1)\nfn=(4) not_executed\n0x30 3 0\n", 13)
        self.assertEqual(self.parse(text), {"callee"})

    def test_missing_or_inconsistent_totals_are_errors(self):
        prefix = "positions: instr\nevents: Ir\nob=/synthetic/libpinyin.so\nfn=f\n0x1 2\n"
        for text in (prefix, prefix + "totals: 3\n", prefix + "calls=1 0x2\ntotals: 2\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.parse(text)

    def test_ir_event_column_and_omitted_trailing_zero(self):
        text = ("positions: instr\nevents: Dr Ir Dw\nob=/synthetic/libpinyin.so\n"
                "fn=f\n0x1 8 2\nfn=zero_ir\n0x2 3\ntotals: 11 2 0\n")
        self.assertEqual(self.parse(text), {"f"})

    def test_unresolved_compression_and_wrong_object_fail(self):
        for body in ("ob=(9)\n", "ob=/synthetic/other.so\nfn=f\n0x1 1 2\n"):
            with self.subTest(body=body), self.assertRaises(ValueError):
                self.parse(profile(body, 2))

    def test_policy_order_is_lexical_with_init_precedence(self):
        render = GENERATOR["policy"]
        first = render("pinyin", {"z", "a"}, {"b", "z"})
        second = render("pinyin", set(reversed(["a", "z"])), {"z", "b"})
        self.assertEqual(first, HEADER + "init\ta\ninit\tz\nsteady\tb\n")
        self.assertEqual(first, second)

    def test_paths_preserve_generics_and_closures(self):
        name = "<crate::Store<T>>::open::{closure#1}"
        self.assertEqual(GENERATOR["canonical"](name + "::h0123456789abcdef.llvm.abcdef"), name)
        self.assertNotEqual(name, "<crate::Store<U>>::open::{closure#1}")

    def test_both_facades_and_profiles_are_mandatory(self):
        with self.assertRaises(ValueError):
            GENERATOR["pairs"](["pinyin=one"], ("pinyin", "zhuyin"), "--library")
        with self.assertRaises(ValueError):
            GENERATOR["pairs"](["pinyin:phrases=one"],
                               tuple(f"{f}:{p}" for f in ("pinyin", "zhuyin")
                                     for p in ("phrases", "journals")), "--init")

    def test_unknown_or_ambiguous_measured_paths_fail(self):
        measured = GENERATOR["measured_paths"]
        table = {"_Rraw": ("crate::open", ".text")}
        with mock.patch.dict(measured.__globals__, callgrind_functions=lambda *_: {"_Rraw"}):
            for available in ({}, {"crate::open": {".text.one", ".text.two"}}):
                with self.subTest(available=available), self.assertRaises(ValueError):
                    measured(Path("-"), LIBRARY, table, available)


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


class RelinkTests(unittest.TestCase):
    def prepare(self, work):
        scripts = {
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
        (work / "version").write_text("LIBPINYIN {\n global:\n pinyin_init;\n local: *;\n};\n")
        (work / "library.a").write_bytes(b"synthetic archive")
        return {**os.environ, "PATH": str(work) + os.pathsep + os.environ["PATH"],
                "TEST_NM_INPUT": str(work / "symbols"), "TEST_LINK_ARGS": str(work / "args"),
                "TEST_QUERIES": str(work / "queries"), "TEST_LINKER_VERSION": "GNU ld (GNU Binutils) 2.47",
                "TEST_LINKER_HELP": "  --section-ordering-file FILE", "TEST_LINK_EXIT": "0",
                "TEST_EXTRA_EXPORT": "0", "TEST_NM_EXIT": "0", "OXPINYIN_FUNCTION_ORDER_POLICY": ""}

    def relink(self, work, environment, policy=None):
        destination = work / "libpinyin.so.15.0.0"
        destination.write_bytes(b"original installed bytes")
        command = ["bash", str(HERE / "relink-versioned.sh"), "--staticlib", str(work / "library.a"),
                   "--ver", str(work / "version"), "--soname", "libpinyin.so.15", "--dest", str(destination),
                   "--cc", str(work / "compiler")]
        if policy is not None:
            command += ["--function-order", str(policy)]
        result = subprocess.run(command, capture_output=True, text=True, check=False, env=environment)
        arguments = (work / "args").read_bytes().split(b"\0")[:-1]
        arguments[arguments.index(b"-o") + 1] = b"OUTPUT"
        self.assertEqual(list(work.glob("*.relink.*")), [], "relink scratch leaked")
        return result, arguments, destination.read_bytes()

    def test_default_has_no_capability_query_and_fallback_preserves_link_arguments(self):
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
