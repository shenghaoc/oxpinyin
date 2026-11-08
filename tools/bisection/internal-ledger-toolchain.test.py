#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Toolchain-backed regression checks for J identity and gcov execution.

These need libclang, the LLVM tools (clang++, llvm-dis, llvm-cxxfilt, opt) and
GCC with gcov 14 or newer, which only a measurement host provides. The file is
in test_runner.HARNESS_EXCLUDED, so just test-tools does not run it. A missing
input fails a test here; nothing skips.
"""

import importlib.util
import re
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from clang_identity import Clang
from render_ledger import align
import gcov_measure

spec = importlib.util.spec_from_file_location("internal_ledger", Path(__file__).with_name("internal-ledger.py"))
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


def require_gcc(test, minimum=14):
    """Fail test with a clear message unless g++ is GCC `minimum` or newer.

    gcov reads only GCC-compiled data, so a Clang g++ or an older GCC cannot
    run the gcov probes.
    """
    try:
        macros = subprocess.run(['g++', '-dM', '-E', '-x', 'c++', '-'], input='', capture_output=True,
                                text=True, timeout=30).stdout
    except (OSError, subprocess.SubprocessError) as error:
        test.fail(f'g++ is required to build the gcov probes, and could not run: {error}')
    defined = dict(re.findall(r'^#define (__GNUC__|__clang__) (\S+)$', macros, re.MULTILINE))
    if '__clang__' in defined:
        test.fail(f'g++ is Clang; the gcov probes need GCC {minimum} or newer')
    major = int(defined.get('__GNUC__', 0))
    if major < minimum:
        test.fail(f'g++ is GCC {major}; the gcov probes need GCC {minimum} or newer')


class IdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory()
        cls.pin = Path(cls.temp.name)
        cls.source = cls.pin / "probe.cpp"
        cls.source.write_text("""
namespace pinyin {
static int helper(int value) { return value + 1; }
static double helper(double value) { return value + 2.0; }
}
extern void receive(int (*callback)(int));
struct Base { virtual ~Base() {} virtual int go() const = 0; };
struct One : Base { ~One() override {} int go() const override { return 1; } };
struct Two : Base { ~Two() override {} int go() const override { return 2; } };
template<unsigned N> int templated(int v) { return v + N; }
template<unsigned N> struct Box { int member(int v) { return N + v; } double member(double v) { return N + v; } };
extern "C" double entry(Base *base) {
    receive(pinyin::helper);
    return pinyin::helper(1.0) + base->go();
}
extern "C" int pinned(One *one) { return one->go(); }
extern "C" void deleted(One *one) { delete one; }
extern "C" int instance() { return templated<2>(1); }
extern "C" int member_instance() { Box<2> value; return value.member(1); }
extern "C" int actual(int value) { return value; }
extern "C" int alias_entry(int value) __attribute__((alias("actual")));
extern "C" int call_alias(int value) { return alias_entry(value); }
""")
        cls.ast, cls.ast_edges, cls.concrete, cls.overrides, _, cls.metadata = Clang().extract(cls.source, ["-std=c++17"], cls.pin)
        cls.bc = cls.pin / "probe.bc"
        ledger.run(["clang++", "-std=c++17", "-O0", "-g", "-fno-inline", "-emit-llvm", "-c", cls.source, "-o", cls.bc])
        cls.nodes, cls.symbols, cls.edges, cls.external, cls.indirect = ledger.llvm(cls.pin, cls.bc, cls.ast)

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_overloads_have_distinct_usr_and_signature(self):
        helpers = [n for n in self.ast.values() if n["name"] == "helper"]
        self.assertEqual(len(helpers), 2)
        self.assertEqual(len({n["usr"] for n in helpers}), 2)
        self.assertEqual(len({n["signature"] for n in helpers}), 2)
        self.assertTrue(all(n["usr"] in self.nodes for n in helpers))

    def test_callback_and_direct_overload_are_both_reachable(self):
        paths = ledger.witness(self.nodes, self.edges, {"entry": self.symbols["entry"]})
        helpers = [n for n in self.ast.values() if n["name"] == "helper"]
        self.assertTrue(all(n["usr"] in paths for n in helpers))
        kinds = {kind for a, b, kind in self.edges if a == self.symbols["entry"]}
        self.assertIn("LLVM-direct", kinds)
        self.assertIn("LLVM-address/callback", kinds)
        self.assertIn("entry", ledger.format_path(self.nodes, paths[self.symbols["entry"]][1]))

    def test_virtual_hierarchy_preserves_pure_base_and_overrides(self):
        methods = [n for n in self.ast.values() if n["name"] == "go"]
        self.assertEqual(len(methods), 3)
        base = next(n["usr"] for n in methods if "Base::" in n["signature"])
        one = next(n["usr"] for n in methods if "One::" in n["signature"])
        two = next(n["usr"] for n in methods if "Two::" in n["signature"])
        self.assertNotIn(base, self.concrete)
        self.assertIn(one, self.concrete)
        self.assertIn(two, self.concrete)
        self.assertEqual({b for a, b in self.overrides if a == base}, {one, two})
        self.assertTrue(any(a == self.symbols["entry"] and b == base and kind == "virtual" for a, b, kind, line in self.ast_edges))
        self.assertTrue(any(a == self.symbols["pinned"] and b == one and kind == "virtual" for a, b, kind, line in self.ast_edges))
        self.assertTrue(any(a == self.symbols["entry"] for a, line in self.indirect))

    def test_unknown_library_body_stays_a_boundary(self):
        self.assertIn((self.symbols["entry"], "_Z7receivePFiiE"), self.external)
        self.assertNotIn("_Z7receivePFiiE", self.symbols)

    def test_static_receiver_and_deleting_destructor_are_recorded(self):
        one = next(n for n in self.ast.values() if n['name'] == 'go' and 'One::' in n['signature'])
        self.assertTrue(any(a == self.symbols['pinned'] and receiver == one['owner_usr'] for a, line, target, receiver in self.metadata['virtual_sites']))
        self.assertTrue(any(a == self.symbols['deleted'] and receiver == one['owner_usr'] for a, line, target, receiver in self.metadata['deletes']))

    def test_function_body_and_compiler_template_origin(self):
        helper = next(n for n in self.ast.values() if n['name'] == 'helper' and n['canonical_type'] == 'int (int)')
        self.assertLessEqual(helper['line'], helper['body_start'])
        self.assertLessEqual(helper['body_start'], helper['body_end'])
        generic = next(n['usr'] for n in self.ast.values() if n['function_kind'] == 30 and n['name'] == 'templated')
        self.assertTrue(any(n.get('template_usr') == generic for n in self.ast.values()))

    def test_class_template_member_origin_does_not_conflate_same_line_overloads(self):
        primary = [n for n in self.ast.values() if n['name'] == 'member' and n.get('definition')]
        self.assertEqual(len(primary), 2)
        integer = next(n['usr'] for n in primary if n['canonical_type'] == 'int (int)')
        floating = next(n['usr'] for n in primary if n['canonical_type'] == 'double (double)')
        instantiated = [n for n in self.nodes.values() if n.get('template_usr') == integer]
        self.assertTrue(instantiated)
        self.assertFalse(any(n.get('template_usr') == floating for n in self.nodes.values()))

    def test_exact_llvm_alias_reaches_its_emitted_body(self):
        self.assertEqual(self.symbols['alias_entry'], self.symbols['actual'])
        self.assertIn((self.symbols['call_alias'], self.symbols['actual'], 'LLVM-direct'), self.edges)
        self.assertNotIn((self.symbols['call_alias'], 'alias_entry'), self.external)


class GcovTests(unittest.TestCase):
    def test_abort_shim_flushes_counters_and_retains_signal(self):
        require_gcc(self)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'abort.cpp'
            source.write_text('#include <cstdlib>\nvoid target() { std::abort(); }\nint main() { target(); }\n')
            shim = root / 'shim.cpp'
            shim.write_text(gcov_measure.FLUSH_SHIM)
            obj, shim_obj, exe = root / 'abort.cpp.o', root / 'shim.o', root / 'abort'
            gcov_measure.command(['g++', '-g', '-O0', '--coverage', '-c', source, '-o', obj])
            gcov_measure.command(['g++', '-c', shim, '-o', shim_obj])
            gcov_measure.command(['g++', '--coverage', obj, shim_obj, '-o', exe])
            import resource
            def no_core(): resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
            result = subprocess.run([exe], capture_output=True, timeout=10, preexec_fn=no_core)
            self.assertEqual(result.returncode, -signal.SIGABRT)
            self.assertIn(b'J-GCOV-ABORT-BEGIN\n', result.stdout)
            self.assertIn(b'J-GCOV-ABORT-END\n', result.stdout)
            counts = gcov_measure.counters(root, root)
            self.assertTrue(any(identity[0] == '_Z6targetv' and count > 0 for identity, count in counts.items()))

    def test_gcov_execution_aligns_by_exact_symbol_extent_and_tu(self):
        require_gcc(self)
        with tempfile.TemporaryDirectory() as temp:
            pin = Path(temp)
            source = pin / 'probe.cpp'
            source.write_text('namespace pinyin { int same(int v) { return v; } double same(double v) { return v; } }\nint main() { return pinyin::same(0); }\n')
            ast, *_ = Clang().extract(source, ['-std=c++17'], pin)
            bc = pin / 'probe.bc'
            ledger.run(['clang++', '-O0', '-g', '-emit-llvm', '-c', source, '-o', bc])
            nodes, *_ = ledger.llvm(pin, bc, ast)
            exe = pin / 'probe'
            obj = pin / 'probe.cpp.o'
            gcov_measure.command(['g++', '-O0', '--coverage', '-c', source, '-o', obj])
            gcov_measure.command(['g++', '--coverage', obj, '-o', exe])
            gcov_measure.command([exe])
            counts = gcov_measure.counters(pin, pin)
            positive = {key: {'MATCH/probe'} for key, count in counts.items() if count > 0}
            matched, unknown = align(nodes, positive, ledger.symbol_base)
            integer = next(n['usr'] for n in ast.values() if n['name'] == 'same' and n['canonical_type'] == 'int (int)')
            floating = next(n['usr'] for n in ast.values() if n['name'] == 'same' and n['canonical_type'] == 'double (double)')
            self.assertIn(integer, matched)
            self.assertNotIn(floating, matched)
            identity = next(key for key in positive if key[0] == nodes[integer]['symbol'])
            wrong = (identity[0], 'other.cpp', *identity[2:])
            self.assertFalse(align(nodes, {wrong: {'MATCH'}}, ledger.symbol_base)[0])


if __name__ == "__main__":
    unittest.main()
