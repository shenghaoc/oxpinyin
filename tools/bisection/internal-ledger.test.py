#!/usr/bin/env python3
"""Compiler-backed regression checks for J identity and ledger sensitivity."""

import importlib.util
import json
import hashlib
import os
import signal
import subprocess
from collections import defaultdict
from types import SimpleNamespace
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from clang_identity import Clang
from coverage_tiers import lexical_mask, rust_items, cited_items
from render_ledger import align, annotate_comparison
import gcov_measure
import compiler_cells
import finish_ledger

spec = importlib.util.spec_from_file_location("internal_ledger", Path(__file__).with_name("internal-ledger.py"))
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


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


class TierTests(unittest.TestCase):
    def test_abort_shim_flushes_counters_and_retains_signal(self):
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

    def test_reviewed_operator_join_requires_extent_signature_and_unit(self):
        identity = ('_Zeq', 'src/storage/chewing_key.h', 81, 91, 'pinyin.cpp')
        node = dict(file=identity[1], line=81, body_end=91,
                    signature='operator==(_ChewingKey, _ChewingKey)',
                    llvm_bindings={('_Zeq11_ChewingKeyS_', 'pinyin.cpp')})
        matched, unknown = align({'actual': node}, {identity: {'MATCH'}}, ledger.symbol_base)
        self.assertEqual(set(matched), {'actual'})
        self.assertFalse(unknown)
        without_extent = dict(node)
        without_extent.pop('body_end')
        self.assertEqual(set(align({'actual': without_extent}, {identity: {'MATCH'}}, ledger.symbol_base)[0]), {'actual'})
        for changes in ({'body_end': 90}, {'signature': 'operator==(int, int)'},
                        {'llvm_bindings': {('_Zeq11_ChewingKeyS_', 'zhuyin.cpp')}},
                        {'llvm_bindings': {('_Zeq5WrongS_', 'pinyin.cpp')}}):
            matched, unknown = align({'actual': dict(node, **changes)}, {identity: {'MATCH'}}, ledger.symbol_base)
            self.assertFalse(matched)
            self.assertEqual(unknown, {identity})

    def test_citations_in_strings_or_test_items_do_not_classify_shipping_items(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source = root / 'lib.rs'
            source.write_text('''const EXAMPLE: &str = r#"// pin.cpp:2-4"#;
/// implements real (pin.cpp:5-7)
pub fn real() { /* pin.cpp:8-9 */ }
#[cfg(test)] mod tests { /// pin.cpp:10-11
fn fake() {} }
#[test] fn other() {}
''')
            mask, comments = lexical_mask(source.read_text())
            self.assertNotIn('pin.cpp:2-4', '\n'.join(c[2] for c in comments))
            items = list(rust_items(source, root))
            self.assertEqual({item['name'] for item in items}, {'real'})

    def test_complete_callback_type_and_binding_count_are_required(self):
        node = {'file': 'external-header/stl_algo.h', 'signature': 'run(bool (*)(pinyin::Record const&, pinyin::Record const&))'}
        prototype = next(iter(ledger.callback_prototypes(node['signature'])))
        self.assertEqual(ledger.singleton_callback(node, {prototype: {'actual-target'}}, set(), True)[0], 'actual-target')
        self.assertIsNone(ledger.singleton_callback(node, {prototype: {'one', 'two'}}, set(), True))
        self.assertIsNone(ledger.singleton_callback(node, {prototype: {'one'}}, {'unknown-binding'}, True))
        self.assertIsNone(ledger.singleton_callback(node, {prototype: {'one'}}, set(), False))
        nodes = {'one': dict(pin=True, function_kind=8, canonical_type='bool(pinyin::Record const&,pinyin::Record const&)'),
                 'two': dict(pin=True, function_kind=21, is_static=True, canonical_type='bool(pinyin::Record const&,pinyin::Record const&)')}
        pool, unknown = ledger.internal_callback_pool(nodes, {('caller', key, 'LLVM-address/callback') for key in nodes})
        self.assertEqual(next(iter(pool.values())), {'one', 'two'})

    def test_gcov_execution_aligns_by_exact_symbol_extent_and_tu(self):
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

    def test_contract_matcher_rejects_equal_fatal_failures(self):
        module = gcov_measure.contract_module()
        spec = dict(abort=module.NO_ABORT, either=False, crash=None, stderr=False)
        fatal = dict(exit=-6, result=None)
        self.assertFalse(module.matches(spec, fatal, fatal))

    def test_measurement_observer_preserves_abort_and_crash_arms(self):
        module = gcov_measure.contract_module()
        spec = dict(abort=False, either=True, crash={'ret': False}, stderr=True, userdir=False)
        reference = dict(exit=-6, result=None, stderr='reference assert pathname')
        measurement = dict(exit=-6, result=None, stderr='measurement assert pathname')
        self.assertTrue(gcov_measure.same_reference(module, spec, reference, measurement))
        reference['exit'] = measurement['exit'] = -11
        self.assertFalse(gcov_measure.same_reference(module, spec, reference, measurement))


class MutationTests(unittest.TestCase):
    def test_exact_pinyin_deletion_does_not_use_zhuyin_overload(self):
        nodes = {
            "pinyin-root": {"file": "src/pinyin.cpp", "line": 326, "name": "pinyin_init"},
            "pinyin-format": {"file": "src/pinyin.cpp", "line": 172, "name": "check_format"},
            "zhuyin-format": {"file": "src/zhuyin.cpp", "line": 126, "name": "check_format"},
        }
        rows = {k: "a distinct ledger row for " + k for k in nodes}
        baseline = dict(rows)
        result = ledger.mutation_check(rows, nodes)
        self.assertEqual(len(result), 2)
        self.assertEqual(rows, baseline)
        incomplete = {k: v for k, v in rows.items() if k != "pinyin-format"}
        ok, missing, extra = ledger.check_rows(rows, incomplete)
        self.assertFalse(ok)
        self.assertEqual(missing, {"pinyin-format"})
        self.assertFalse(extra)

    def test_extra_row_is_not_complete(self):
        self.assertFalse(ledger.check_rows({"root": "row"}, {"root": "row", "invented": "row"})[0])


class ReviewGuardTests(unittest.TestCase):
    def test_partial_citation_exclusion_is_identity_and_source_specific(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); pin = root / 'pin'
            src = pin / 'src/storage/phrase_index.cpp'
            src.parent.mkdir(parents=True); src.write_text('configured pin source')
            rust = root / 'crates/oxpinyin-user/src/store.rs'
            rust.parent.mkdir(parents=True); rust.write_text('shipping item')
            key = 'c:@N@pinyin@S@SubPhraseIndex@F@add_unigram_frequency#i#i#'
            nodes = {key: dict(file='src/storage/phrase_index.cpp', name='add_unigram_frequency',
                               usr=key, body_start=150, body_end=178)}
            item = dict(file='crates/oxpinyin-user/src/store.rs', name='add_unigram_frequency',
                        line=562, end=580, comment_line=552,
                        comment='SubPhraseIndex::add_unigram_frequency (phrase_index.cpp:150-178)')
            with patch('coverage_tiers.shipping_crates', return_value={rust.parent.parent}), \
                 patch('coverage_tiers.rust_items', return_value=[item]):
                self.assertFalse(cited_items(root, pin, nodes, tightened='role'))
            alternate = dict(item, name='whole_port',
                             comment='Implements SubPhraseIndex::add_unigram_frequency (phrase_index.cpp:150-178)')
            with patch('coverage_tiers.shipping_crates', return_value={rust.parent.parent}), \
                 patch('coverage_tiers.rust_items', return_value=[alternate]):
                self.assertIn(key, cited_items(root, pin, nodes, tightened='role'))

    def test_rebased_worker_preserves_extra_environment_and_timeout(self):
        module = gcov_measure.contract_module()
        with tempfile.TemporaryDirectory() as scratch:
            with patch.object(module.subprocess, 'Popen') as run:
                process = run.return_value
                process.returncode = 0
                process.communicate.return_value = (b'{"ret": true}\n', b'')
                observed = module.run_worker('pinyin', Path('/p'), Path('/data'), 'probe', scratch,
                                             extra_env={'J_TEST_ENV': 'retained'}, timeout=60)
            self.assertEqual(run.call_args.kwargs['env']['J_TEST_ENV'], 'retained')
            self.assertEqual(process.communicate.call_count, 1)
            self.assertGreater(process.communicate.call_args.kwargs['timeout'], 0)
            self.assertLessEqual(process.communicate.call_args.kwargs['timeout'], 0.1)
            self.assertTrue(run.call_args.kwargs['start_new_session'])
            self.assertEqual(run.call_args.args[0][1], str(module.WORKER))
            self.assertEqual(observed['result'], {'ret': True})

    def test_default_sweep_matches_ordinary_driver_scope(self):
        good = dict(exit=0, result={}, stderr='', userfiles=[], measurement_abort_flush=False)
        cases = {name: dict(cells=['bdb'], mode='pinyin') for name in ('ordinary', 'axis-c-owned')}
        module = SimpleNamespace(CELL='bdb', CASES=cases, __file__='driver', run_worker=lambda *a, **kw:dict(good),
                                 matches=lambda *a:True)
        with patch.object(gcov_measure, 'verify_prefix'), patch.object(gcov_measure, 'contract_module', return_value=module), \
             patch.object(gcov_measure, 'clear'), patch.object(gcov_measure, 'counters', return_value={}), \
             patch.object(gcov_measure, 'same_reference', return_value=True):
            result = gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'))
        self.assertEqual(result[3], [('ordinary', 'MATCH')])

    def test_resume_replays_only_invalid_cases_and_preserves_completed_credit(self):
        behavior = defaultdict(set, valid={'contract/complete'}, stale={'contract/paused'},
                               shared={'contract/complete', 'contract/paused'})
        previous = (behavior, {'old-bad'}, {'contract/paused'},
                    [('complete', 'MATCH'), ('paused', 'TIMEOUT')])
        fresh = (defaultdict(set, repaired={'contract/paused'}), set(), set(), [('paused', 'MATCH')])
        with patch.object(gcov_measure, 'contracts', return_value=fresh) as replay:
            result = gcov_measure.reconcile_interrupted(previous, ['paused'], *([Path('/unused')] * 5))
        self.assertEqual(replay.call_args.kwargs['names'], {'paused'})
        self.assertEqual(dict(result[0]), {'valid': {'contract/complete'},
                                        'shared': {'contract/complete'},
                                        'repaired': {'contract/paused'}})
        self.assertFalse(result[1]); self.assertFalse(result[2])
        self.assertEqual(result[3], [('complete', 'MATCH'), ('paused', 'MATCH')])
        with self.assertRaisesRegex(RuntimeError, 'identities absent'):
            gcov_measure.reconcile_interrupted(previous, ['unknown'], *([Path('/unused')] * 5))

    def test_surface_shim_markers_are_validated_separately(self):
        row = b'["input", 1, true, true, [], []]\n'
        markers = b'J-GCOV-ABORT-BEGIN\nJ-GCOV-ABORT-END\n'
        records, aborts = gcov_measure.surface_records(markers + row)
        self.assertEqual(records, [['input', 1, True, True, [], []]])
        self.assertEqual(aborts, 1)
        for malformed in (b'unknown diagnostic\n' + row, b'J-GCOV-ABORT-BEGIN\n' + row,
                          b'J-GCOV-ABORT-END\n' + row, b'J-GCOV-ABORT-BEGIN\n',
                          b'[]\n', b'J-GCOV-ABORT-BEGIN\n' + markers):
            with self.assertRaises(RuntimeError): gcov_measure.surface_records(malformed)

    def test_each_oracle_binary_is_authenticated(self):
        with tempfile.TemporaryDirectory() as temp:
            prefix = Path(temp)
            (prefix / 'lib').mkdir()
            provenance = {}
            for name in ('libpinyin', 'libzhuyin'):
                body = ('pinned ' + name).encode()
                (prefix / f'lib/{name}.so').write_bytes(body)
                provenance[name] = hashlib.sha256(body).hexdigest()
            (prefix / 'oracle-pin.txt').write_text('shared_object_sha256=' + provenance['libpinyin'] + '\n')
            self.assertEqual(gcov_measure.oracle_hashes(prefix, provenance), provenance)
            for name in provenance:
                binary = prefix / f'lib/{name}.so'
                original = binary.read_bytes()
                binary.write_bytes(b'tampered')
                with self.assertRaisesRegex(RuntimeError, name + ' oracle SHA'):
                    gcov_measure.oracle_hashes(prefix, provenance)
                binary.write_bytes(original)

    def test_cell_is_bdb_before_fixture_registration(self):
        for inherited in ('kc', 'tkrzw'):
            with patch.dict(os.environ, CONTRACT_DIFF_CELL=inherited):
                module = gcov_measure.contract_module()
                self.assertEqual(module.CELL, 'bdb')
                self.assertEqual(os.environ['CONTRACT_DIFF_CELL'], 'bdb')
                with patch.object(module, '_craft_bdb') as bdb, patch.object(module, '_craft_kc') as kc, \
                     patch.object(module, '_craft_tkrzw') as tkrzw:
                    module._craft_db('probe', [('key', 'value')], 1)
                    bdb.assert_called_once_with('probe', [('key', 'value')], 1, keep=False)
                    kc.assert_not_called(); tkrzw.assert_not_called()

    def test_gate_rejects_zero_filtered_failed_or_multiple_executions(self):
        name = gcov_measure.GATE_TESTS[0]
        valid = f'test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out;\n'
        self.assertEqual(gcov_measure.gate_summary(valid, name), 1)
        for bad in (valid.replace('1 passed', '0 passed'), valid.replace('1 passed', '3 passed'),
                    valid.replace('0 failed', '1 failed'), valid.replace('0 ignored', '1 ignored'),
                    valid.replace(name, 'different_test'), valid + valid):
            with self.assertRaises(RuntimeError): gcov_measure.gate_summary(bad, name)

    def test_all_three_contract_sides_timeout_without_credit(self):
        good = dict(exit=0, result={}, stderr='', userfiles=[], measurement_abort_flush=False)
        for timeout_side in range(3):
            calls = []
            def worker(*args, **kwargs):
                calls.append(kwargs['timeout'])
                if len(calls) - 1 == timeout_side:
                    raise subprocess.TimeoutExpired('probe', kwargs['timeout'])
                return dict(good)
            module = SimpleNamespace(CELL='bdb', CASES={'probe': dict(cells=['bdb'], mode='pinyin')},
                                     run_worker=worker, __file__='driver', observed=lambda x:x)
            with patch.object(gcov_measure, 'verify_prefix'), patch.object(gcov_measure, 'contract_module', return_value=module), \
                 patch.object(gcov_measure, 'clear'), patch.object(gcov_measure, 'counters', return_value={('body',): 1}), \
                 patch.object(gcov_measure, 'same_reference', return_value=True):
                result = gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'))
            self.assertEqual(calls, [60, 60, 60])
            self.assertFalse(result[0])
            self.assertIn('contract/probe', result[2])
            self.assertEqual(result[3], [('probe', 'TIMEOUT')])
            self.assertEqual(module.__file__, 'driver')

    def test_prefix_failure_prevents_driver_or_native_comparison(self):
        with patch.object(gcov_measure, 'verify_prefix', side_effect=RuntimeError('libzhuyin oracle SHA mismatch')), \
             patch.object(gcov_measure, 'contract_module') as driver:
            with self.assertRaisesRegex(RuntimeError, 'libzhuyin'):
                gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'))
            driver.assert_not_called()

    def test_gate_executes_each_exact_test_and_requires_its_summary(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp)
            deps = target / 'debug/deps'
            deps.mkdir(parents=True)
            binary = deps / 'sentence_surface_parity-probe'
            binary.touch(); binary.chmod(0o700)
            called = []
            def run(argv, **kwargs):
                called.append(argv)
                name = argv[2]
                return SimpleNamespace(returncode=0, stdout=(f'test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out;\n').encode())
            with patch.object(gcov_measure.subprocess, 'run', side_effect=run):
                self.assertIn('four tests PASS', gcov_measure.gates(target, Path('/export'), Path('/model')))
            self.assertEqual([argv[2] for argv in called], list(gcov_measure.GATE_TESTS))
            self.assertTrue(all(argv[1:] == ['--exact', argv[2], '--include-ignored', '--nocapture'] for argv in called))

    def test_missing_synthetic_descriptor_rejects_present_body(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            (out / 'probe.cpp.o').touch()
            nodes = {f'k{i}': dict(file='src/probe.cpp', llvm_bindings={(f's{i}', 'probe.cpp')}) for i in range(15)}
            reviews = {key: ('ABSENT_INLINE', [], 'review') for key in nodes}
            bodies = '\n'.join(f'0 T s{i}' for i in range(15))
            counters = '\n'.join(f'0 d __gcov_.s{i}' for i in range(15))
            with patch.object(finish_ledger, 'IDENTITY_REVIEWS', reviews), patch.object(gcov_measure, 'command', return_value=bodies):
                with self.assertRaisesRegex(RuntimeError, 'gcov descriptor missing'):
                    finish_ledger.verify_implicit_objects(nodes, out, lambda x:x)
            with patch.object(finish_ledger, 'IDENTITY_REVIEWS', reviews), patch.object(gcov_measure, 'command', return_value=bodies + '\n' + counters):
                self.assertEqual(len(finish_ledger.verify_implicit_objects(nodes, out, lambda x:x)), 15)

    def test_same_identity_corrupted_compiler_cells_are_rejected(self):
        nodes = {'root': dict(file='src/pinyin.cpp', line=1, signature='root()', usr='root', name='root'),
                 'helper': dict(file='src/helper.cpp', line=2, signature='helper(int)', usr='helper', name='helper')}
        paths = {'root': ('entry', [('root', 'export')]),
                 'helper': ('entry', [('root', 'export'), ('helper', 'LLVM-direct')])}
        state = dict(nodes=nodes, paths=paths, direct_paths=paths, pinned_paths={}, fields=[], ast={},
                     sites=[('helper', 3)], ambiguous=[], resolved_sites=[], singleton_callbacks=[], external={('root', 'outside')})
        rows = [compiler_cells.function_cells(ledger, state, key) + ['T5', '', 'manual'] for key in nodes]
        rows += compiler_cells.boundary_cells(ledger, state)
        self.assertEqual(compiler_cells.validate(ledger, state, nodes, rows), 4)
        for row_index in range(len(rows)):
            columns = [0, 1, 2, 4, 5] + ([8] if row_index >= 2 else [])
            for column in columns:
                changed = [list(row) for row in rows]
                changed[row_index][column] += ' corrupted'
                self.assertEqual(changed[row_index][3], rows[row_index][3])
                with self.assertRaisesRegex(RuntimeError, 'compiler-derived'):
                    compiler_cells.validate(ledger, state, nodes, changed)

    def test_row_comparison_reports_all_changed_fields(self):
        old = '| function/direct | pin | f(int) | usr | root | path | T5 | old MATCH | reason |\n'
        new = '| function/direct | new pin | f(double) | usr | root | new path | BEHAVIOR-COVERED | new MATCH | new reason |\n'
        text = annotate_comparison(new, old, '2da3f089')
        for column in ('pin source', 'signature', 'path/resolution', 'classification',
                       'credited differential', 'counterpart/reason'):
            self.assertIn(column + ' previously:', text)
        self.assertNotIn('identity previously:', text)
        with self.assertRaisesRegex(RuntimeError, 'removed rows'):
            annotate_comparison('', old, '2da3f089')


class CellTests(unittest.TestCase):
    MAKEFILE = """pinyin_SOURCES = \\
               storage/shared_a.cpp \\
               lookup/shared_b.cpp \\
               $(NULL)

if BERKELEYDB
pinyin_SOURCES += storage/only_bdb.cpp
endif

if KYOTOCABINET
pinyin_SOURCES += storage/only_kyotodb.cpp \\
                  storage/also_kyotodb.cpp
endif

if TKRZW
pinyin_SOURCES += storage/only_tkrzwdb.cpp
endif

libpinyin_la_SOURCES = $(pinyin_SOURCES) pinyin.cpp
"""

    def write_pin(self, root):
        (root / 'src/storage').mkdir(parents=True)
        (root / 'src/Makefile.am').write_text(self.MAKEFILE)
        for name in ('ngram_kyotodb.h', 'ngram_tkrzwdb.h', 'ngram_bdb.h', 'ngram.h', 'kyotodb_utils.h', 'shared_utils.h'):
            (root / 'src/storage' / name).write_text('')
        (root / 'src/storage/only_kyotodb.cpp').write_text('#include "kyotodb_utils.h"\n#include "shared_utils.h"\n')
        (root / 'src/storage/also_kyotodb.cpp').write_text('  # include "kyotodb_utils.h"\n')
        (root / 'src/storage/shared_a.cpp').write_text('#include "shared_utils.h"\n#include "ngram.h"\n')
        (root / 'src/storage/ngram.h').write_text('#ifdef HAVE_KYOTO_CABINET\n#include "ngram_kyotodb.h"\n#endif\n')

    def test_cell_sources_select_exactly_one_conditional(self):
        with tempfile.TemporaryDirectory() as temp:
            pin = Path(temp); self.write_pin(pin)
            shared = ['lookup/shared_b.cpp', 'storage/shared_a.cpp']
            self.assertEqual(gcov_measure.cell_sources(pin, 'bdb'), sorted(shared + ['storage/only_bdb.cpp']))
            self.assertEqual(gcov_measure.cell_sources(pin), gcov_measure.cell_sources(pin, 'bdb'))
            self.assertEqual(ledger.sources(pin), gcov_measure.cell_sources(pin, 'bdb'))
            self.assertEqual(gcov_measure.cell_sources(pin, 'kc'), sorted(shared + ['storage/also_kyotodb.cpp', 'storage/only_kyotodb.cpp']))
            self.assertEqual(gcov_measure.cell_sources(pin, 'tkrzw'), sorted(shared + ['storage/only_tkrzwdb.cpp']))
            import delta_ledger
            self.assertEqual(delta_ledger.conditional_files(pin, 'kc'),
                             {'src/storage/also_kyotodb.cpp', 'src/storage/only_kyotodb.cpp', 'src/storage/ngram_kyotodb.h', 'src/storage/kyotodb_utils.h'})
            self.assertEqual(delta_ledger.conditional_files(pin, 'bdb'), {'src/storage/only_bdb.cpp', 'src/storage/ngram_bdb.h'})
            self.assertEqual(delta_ledger.conditional_files(pin, 'tkrzw'), {'src/storage/only_tkrzwdb.cpp', 'src/storage/ngram_tkrzwdb.h'})

    def test_configured_libraries_come_from_config_status(self):
        with tempfile.TemporaryDirectory() as temp:
            status = Path(temp) / 'config.status'
            status.write_text('S["LIBS"]="-lkyotocabinet "\nS["CXXFLAGS"]="-g -O2"\n')
            self.assertEqual(gcov_measure.configured_libraries(status, 'kc'), ['-lkyotocabinet'])
            with self.assertRaisesRegex(RuntimeError, 'do not name the bdb'):
                gcov_measure.configured_libraries(status, 'bdb')
            status.write_text('S["CXXFLAGS"]="-g -O2"\n')
            with self.assertRaisesRegex(RuntimeError, 'configured LIBS'):
                gcov_measure.configured_libraries(status, 'kc')

    def test_contract_module_selects_the_requested_cell_explicitly(self):
        with patch.dict(os.environ, CONTRACT_DIFF_CELL='bdb'):
            module = gcov_measure.contract_module('kc')
            self.assertEqual(module.CELL, 'kc')
            self.assertEqual(os.environ['CONTRACT_DIFF_CELL'], 'kc')
            with patch.object(module, '_craft_bdb') as bdb, patch.object(module, '_craft_kc') as kc, \
                 patch.object(module, '_craft_tkrzw') as tkrzw:
                module._craft_db('probe', [('key', 'value')], 1)
                kc.assert_called_once_with('probe', [('key', 'value')], keep=False)
                bdb.assert_not_called(); tkrzw.assert_not_called()
        with self.assertRaisesRegex(RuntimeError, 'unknown DBM cell'):
            gcov_measure.contract_module('lmdb')

    def test_verify_prefix_checks_the_cell_manifest(self):
        with tempfile.TemporaryDirectory() as temp:
            prefix = Path(temp); (prefix / 'lib').mkdir()
            expected = dict(line.split('=', 1) for line in (gcov_measure.REPO / 'tools/oracle/oracle-pin.txt').read_text().splitlines() if '=' in line and not line.startswith('#'))
            provenance = {}
            for name in ('libpinyin', 'libzhuyin'):
                body = (name + ' kc').encode(); (prefix / f'lib/{name}.so').write_bytes(body)
                provenance[name] = hashlib.sha256(body).hexdigest()
            ref = f"libpinyin-{expected['libpinyin_tag']}-{gcov_measure.PIN}+model20-{expected['model_archive_sha256']}+dbm-kc"
            manifests = {}
            for name, field in [('oracle-data.sha256', 'data_manifest_sha256'), ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')]:
                (prefix / name).write_bytes(b''); manifests[field] = hashlib.sha256(b'').hexdigest()
            (prefix / 'oracle-pin.txt').write_text(f"pin_ref={ref}\ndbm=KyotoCabinet\nshared_object_sha256={provenance['libpinyin']}\n" + ''.join(f'{k}={v}\n' for k, v in manifests.items()))
            self.assertEqual(gcov_measure.verify_prefix(prefix, provenance, 'kc'), 0)
            with self.assertRaises(AssertionError):
                gcov_measure.verify_prefix(prefix, provenance, 'bdb')
            with self.assertRaises(AssertionError):
                gcov_measure.verify_prefix(prefix, provenance, 'tkrzw')

    def test_measurement_worker_requires_an_explicit_cell(self):
        environment = {k: v for k, v in os.environ.items() if k != 'J_MEASUREMENT_CELL'}
        with patch.dict(os.environ, environment, clear=True), patch.object(gcov_measure, 'contract_module') as module:
            with self.assertRaises(KeyError):
                gcov_measure.worker('pinyin', '/so', '/data', 'probe')
            module.assert_not_called()
        with patch.dict(os.environ, J_MEASUREMENT_CELL='tkrzw'), \
             patch.object(gcov_measure, 'contract_module', side_effect=RuntimeError('selected')) as module:
            with self.assertRaisesRegex(RuntimeError, 'selected'):
                gcov_measure.worker('pinyin', '/so', '/data', 'probe')
            module.assert_called_once_with('tkrzw')

    def test_measured_worker_passes_the_cell_to_its_subprocess(self):
        module = gcov_measure.contract_module('kc')
        with tempfile.TemporaryDirectory() as scratch:
            result = SimpleNamespace(returncode=0, stdout=b'{"ret": true}\n', stderr=b'')
            with patch.object(module.subprocess, 'run', return_value=result) as run:
                gcov_measure.measured_worker(module, 'pinyin', Path('/p'), Path('/data'), 'probe', scratch, 'kc')
            self.assertEqual(run.call_args.kwargs['env']['J_MEASUREMENT_CELL'], 'kc')
            self.assertEqual(run.call_args.kwargs['env']['CONTRACT_DIFF_CELL'], 'kc')
            self.assertEqual(run.call_args.args[0][1], str(Path(gcov_measure.__file__).resolve()))

    def test_contracts_skip_other_cells_fixtures_and_select_the_cell(self):
        good = dict(exit=0, result={}, stderr='', userfiles=[], measurement_abort_flush=False)
        cases = {'shared': dict(cells=('bdb', 'kc', 'tkrzw'), mode='pinyin'), 'bdb-only': dict(cells=('bdb',), mode='pinyin')}
        module = SimpleNamespace(CELL='kc', CASES=cases, __file__='driver', run_worker=lambda *a, **kw: dict(good), matches=lambda *a: True)
        with patch.object(gcov_measure, 'verify_prefix') as verify, patch.object(gcov_measure, 'contract_module', return_value=module) as select, \
             patch.object(gcov_measure, 'clear'), patch.object(gcov_measure, 'counters', return_value={}), \
             patch.object(gcov_measure, 'same_reference', return_value=True):
            result = gcov_measure.contracts(Path('/unused'), Path('/p'), Path('/z'), Path('/out'), Path('/pin'), cell='kc', provenance={'x': 'y'})
        self.assertEqual(result[3], [('bdb-only', 'SKIP'), ('shared', 'MATCH')])
        select.assert_called_once_with('kc')
        verify.assert_called_once_with(Path('/unused'), {'x': 'y'}, 'kc')

    def test_surface_workers_receive_the_cell(self):
        row = b'["input", 1, true, true, [], []]\n'
        calls = []
        def run(argv, **kwargs):
            calls.append(argv); return SimpleNamespace(returncode=0, stdout=row, stderr=b'')
        with patch.object(gcov_measure, 'verify_prefix'), patch.object(gcov_measure, 'clear'), \
             patch.object(gcov_measure, 'counters', return_value={('f',): 1}), patch.object(gcov_measure.subprocess, 'run', side_effect=run):
            attributed, nonmatch, incomplete, results = gcov_measure.surfaces(Path('/prefix'), Path('/p'), Path('/out'), Path('/pin'), 'tkrzw', {'x': 'y'})
        self.assertEqual(len(calls), 12)
        self.assertTrue(all(argv[-1] == 'tkrzw' and argv[-5] == '--surface-worker' for argv in calls))
        self.assertEqual(results, [('sentence', 1, 1, 0), ('ties', 1, 1, 0), ('corpus', 1, 1, 0)])

    def test_delta_partition_and_break_point(self):
        import delta_ledger
        nodes = {
            'root': dict(file='src/pinyin.cpp', line=1, signature='pinyin_init()', usr='c:@F@pinyin_init', name='pinyin_init', pin=True, definition=True),
            'shared': dict(file='src/storage/ngram.cpp', line=5, signature='shared()', usr='c:@F@shared', name='shared', pin=True, definition=True),
            'only-here': dict(file='src/lookup/lookup.cpp', line=9, signature='only_here()', usr='c:@F@only_here', name='only_here', pin=True, definition=True),
            'cell': dict(file='src/storage/ngram_kyotodb.cpp', line=3, signature='cell()', usr='c:@F@cell', name='cell', pin=True, definition=True),
            'cell-dead': dict(file='src/storage/ngram_kyotodb.cpp', line=40, signature='dead()', usr='c:@F@dead', name='dead', pin=True, definition=True),
            'cell-indirect': dict(file='src/storage/ngram_kyotodb.h', line=12, signature='indirect()', usr='c:@F@indirect', name='indirect', pin=True, definition=True),
            'unreached': dict(file='src/storage/ngram.cpp', line=20, signature='unreached()', usr='c:@F@unreached', name='unreached', pin=True, definition=True),
            'header': dict(file='external-header/stl.h', line=1, signature='std::sort()', usr='', name='sort', pin=False, definition=True),
            'signature:src/storage/chewing_key.h:81:operator==(_ChewingKey, _ChewingKey) (.4245)': dict(file='src/storage/chewing_key.h', line=81, signature='operator==(_ChewingKey, _ChewingKey) (.4245)', usr='', name='operator==(_ChewingKey, _ChewingKey) (.4245)', pin=True, definition=True, llvm_bindings={('_Zeq11_ChewingKeyS_', 'pinyin.cpp')}),
        }
        reachable = {key: nodes[key] for key in ('root', 'shared', 'only-here', 'cell', 'signature:src/storage/chewing_key.h:81:operator==(_ChewingKey, _ChewingKey) (.4245)')}
        bdb = {
            'c:@F@pinyin_init': dict(file='src/pinyin.cpp', line=1, path='pinyin_init() @ src/pinyin.cpp:1 (root)'),
            'signature:src/storage/chewing_key.h:81:operator==(_ChewingKey, _ChewingKey) (.4027)': dict(file='src/storage/chewing_key.h', line=81, path='pinyin_init() @ src/pinyin.cpp:1 (root) → operator==(_ChewingKey, _ChewingKey) (.4027) @ src/storage/chewing_key.h:81 (LLVM-direct)'),
            'c:@F@shared': dict(file='src/storage/ngram.cpp', line=5, path='pinyin_init() @ src/pinyin.cpp:1 (root) → shared() @ src/storage/ngram.cpp:5 (AST-direct)'),
            'c:@F@unreached': dict(file='src/storage/ngram.cpp', line=20, path='pinyin_init() @ src/pinyin.cpp:1 (root) → bdb_only() @ src/storage/ngram_bdb.cpp:7 (AST-direct) → unreached() @ src/storage/ngram.cpp:20 (AST-direct)'),
            'c:@F@bdb_only': dict(file='src/storage/ngram_bdb.cpp', line=7, path='pinyin_init() @ src/pinyin.cpp:1 (root) → bdb_only() @ src/storage/ngram_bdb.cpp:7 (AST-direct)'),
        }
        for ident, row in bdb.items():
            row['identity'] = ident
        state = dict(nodes=nodes)
        cell_files = {'src/storage/ngram_kyotodb.cpp', 'src/storage/ngram_kyotodb.h'}
        bdb_files = {'src/storage/ngram_bdb.cpp', 'src/storage/ngram_bdb.h'}
        rows, missing = delta_ledger.partition(state, bdb, cell_files, bdb_files, reachable, executed={'cell-indirect': {'contract/x'}})
        self.assertEqual(rows, {'cell': 'a', 'cell-indirect': 'a', 'only-here': 'b+'})
        self.assertEqual(set(missing), {'c:@F@unreached'})
        clone = 'signature:src/storage/chewing_key.h:81:operator==(_ChewingKey, _ChewingKey) (.4027)'
        self.assertEqual(delta_ledger.carrier(state, bdb, bdb_files, clone), 'signature:src/storage/chewing_key.h:81:operator==(_ChewingKey, _ChewingKey) (.4245)')
        self.assertEqual(delta_ledger.carrier(state, bdb, bdb_files, 'c:@F@unreached'), 'unreached')
        self.assertIsNone(delta_ledger.carrier(state, bdb, bdb_files, 'c:@F@bdb_only'))
        self.assertIn('step 2 `bdb_only()` @ src/storage/ngram_bdb.cpp:7 is not defined', delta_ledger.break_point(state, reachable, bdb['c:@F@unreached']))
        mutant = {key: node for key, node in reachable.items() if key != 'root'}
        rows, missing = delta_ledger.partition(state, bdb, cell_files, bdb_files, mutant, executed={})
        self.assertEqual(set(missing), {'c:@F@unreached', 'c:@F@pinyin_init'})
        self.assertIn('has no export witness in this cell', delta_ledger.break_point(state, mutant, bdb['c:@F@pinyin_init']))
        self.assertIn('call edge itself is absent', delta_ledger.break_point(state, reachable, dict(path='pinyin_init() @ src/pinyin.cpp:1 (root) → shared() @ src/storage/ngram.cpp:5 (AST-direct)')))

    def clone_fixture(self, units):
        """bdb's three operator== clones (as at the pin) and a cell with one clone per given unit."""
        sig = 'operator==(_ChewingKey, _ChewingKey)'
        base = f'signature:src/storage/chewing_key.h:81:{sig}'
        nodes = {
            'pkm': dict(file='src/storage/phonetic_key_matrix.cpp', line=169, signature='pinyin::inner_split_step()', usr='c:@F@inner_split_step', name='inner_split_step', pin=True, definition=True),
            'zr': dict(file='src/zhuyin.cpp', line=2026, signature='zhuyin_get_right_zhuyin_offset()', usr='c:@F@zhuyin_get_right_zhuyin_offset', name='zhuyin_get_right_zhuyin_offset', pin=True, definition=True),
            'pr': dict(file='src/pinyin.cpp', line=3063, signature='pinyin_get_right_pinyin_offset()', usr='c:@F@pinyin_get_right_pinyin_offset', name='pinyin_get_right_pinyin_offset', pin=True, definition=True),
        }
        for index, unit in enumerate(units):
            suffix = '' if index == 0 else f' (.{4200 + index})'
            nodes[base + suffix] = dict(file='src/storage/chewing_key.h', line=81, signature=sig + suffix, usr='', name=sig + suffix,
                                        pin=True, definition=True, llvm_bindings={('_Zeq11_ChewingKeyS_', unit)})
        bdb = {}
        for suffix, caller in (('', 'pinyin::inner_split_step() @ src/storage/phonetic_key_matrix.cpp:169 (AST-direct)'),
                               (' (.4027)', 'zhuyin_get_right_zhuyin_offset() @ src/zhuyin.cpp:2026 (root)'),
                               (' (.4037)', 'pinyin_get_right_pinyin_offset() @ src/pinyin.cpp:3063 (root)')):
            bdb[base + suffix] = dict(identity=base + suffix, file='src/storage/chewing_key.h', line=81, signature=sig + suffix,
                                      path=f'{caller} → {sig}{suffix} @ src/storage/chewing_key.h:81 (LLVM-direct)')
        for key in ('pkm', 'zr', 'pr'):
            bdb[nodes[key]['usr']] = dict(identity=nodes[key]['usr'], file=nodes[key]['file'], line=nodes[key]['line'],
                                          signature=nodes[key]['signature'], path=f"{nodes[key]['signature']} @ {nodes[key]['file']}:{nodes[key]['line']} (root)")
        return base, dict(nodes=nodes), bdb

    def test_clone_families_match_one_to_one_by_translation_unit(self):
        import delta_ledger
        base, state, bdb = self.clone_fixture(['phonetic_key_matrix.cpp', 'zhuyin.cpp', 'pinyin.cpp'])
        nodes = state['nodes']
        families = delta_ledger.clone_families(state, bdb, set())
        self.assertEqual(families, {base})
        rows, missing = delta_ledger.partition(state, bdb, set(), set(), dict(nodes), executed={})
        self.assertEqual((rows, missing), ({}, {}))
        pairs, _ = delta_ledger.match_bdb(state, bdb, set(), list(nodes), families)
        self.assertEqual(pairs[base + ' (.4027)'], base + ' (.4201)')
        self.assertEqual(pairs[base + ' (.4037)'], base + ' (.4202)')
        self.assertEqual(pairs[base], base)
        # One cell clone left: the two other bdb clones are delta-b- rows, not inherited through it.
        reachable = {key: node for key, node in nodes.items() if key not in (base + ' (.4201)', base + ' (.4202)')}
        rows, missing = delta_ledger.partition(state, bdb, set(), set(), reachable, executed={})
        self.assertEqual(rows, {})
        self.assertEqual(set(missing), {base + ' (.4027)', base + ' (.4037)'})
        self.assertEqual(delta_ledger.carrier(state, bdb, set(), base + ' (.4027)'), base + ' (.4201)')
        self.assertIn('has no export witness', delta_ledger.break_point(state, reachable, missing[base + ' (.4027)'], families))
        # A clone compiled into another unit is a different row on both sides.
        base, state, bdb = self.clone_fixture(['phonetic_key_matrix.cpp', 'chewing_large_table2_kyotodb.cpp', 'pinyin.cpp'])
        rows, missing = delta_ledger.partition(state, bdb, set(), set(), dict(state['nodes']), executed={})
        self.assertEqual(rows, {base + ' (.4201)': 'b+'})
        self.assertEqual(set(missing), {base + ' (.4027)'})
        self.assertIsNone(delta_ledger.carrier(state, bdb, set(), base + ' (.4027)'))
        self.assertIn('has no clone in translation unit zhuyin.cpp', delta_ledger.break_point(state, state['nodes'], missing[base + ' (.4027)'], families))
        # Two cell clones in one unit cannot be paired one to one.
        base, state, bdb = self.clone_fixture(['phonetic_key_matrix.cpp', 'zhuyin.cpp', 'zhuyin.cpp'])
        with self.assertRaisesRegex(RuntimeError, 'ambiguous in translation unit'):
            delta_ledger.partition(state, bdb, set(), set(), dict(state['nodes']), executed={})
        # A clone whose caller is header-defined has no derivable unit.
        base, state, bdb = self.clone_fixture(['phonetic_key_matrix.cpp', 'zhuyin.cpp', 'pinyin.cpp'])
        bdb[base + ' (.4027)']['path'] = 'pinyin::PhoneticKeyMatrix::size() @ src/storage/phonetic_key_matrix.h:40 (root) → operator==(_ChewingKey, _ChewingKey) (.4027) @ src/storage/chewing_key.h:81 (LLVM-direct)'
        with self.assertRaisesRegex(RuntimeError, 'not derivable'):
            delta_ledger.partition(state, bdb, set(), set(), dict(state['nodes']), executed={})

    def test_declared_residuals_follow_the_rust_gate_source(self):
        names = gcov_measure.declared_residuals()
        self.assertEqual(len(names), 6)
        text = (gcov_measure.REPO / 'docs/findings/sentence-surface.md').read_text()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); (root / 'docs/findings').mkdir(parents=True)
            doc = root / 'docs/findings/sentence-surface.md'
            doc.write_text(text)
            self.assertEqual(gcov_measure.declared_residuals(root), names)
            first = sorted(names)[0]
            row = next(line for line in text.splitlines() if line.startswith('| `' + first + '`') or line.startswith('| ' + first + ' '))
            doc.write_text(text.replace(row + '\n', row + '\n' + row.replace(first, first + 'x', 1) + '\n', 1))
            with self.assertRaisesRegex(RuntimeError, 'count moved'):
                gcov_measure.declared_residuals(root)
            doc.write_text(text.replace('Only the following six shared-comparison rows are class (a), as ruled.', 'moved'))
            with self.assertRaisesRegex(RuntimeError, 'declaration moved'):
                gcov_measure.declared_residuals(root)

    def test_surfaces_reject_differences_outside_the_declared_residuals(self):
        def rows(texts, changed=()):
            return b''.join(json.dumps([text, 1, True, True, ['other' if text in changed else 'same'], []]).encode() + b'\n' for text in texts)
        texts = ['residual', 'plain', 'other']
        def run_with(subject_changed=(), measured_changed=()):
            def run(argv, **kwargs):
                so = argv[4]
                if kwargs.get('input') is not None:
                    return SimpleNamespace(returncode=0, stdout=rows([t for t in texts if t in json.loads(kwargs['input'])]), stderr=b'')
                changed = subject_changed if so == '/p' else measured_changed if so.startswith('/out') else ()
                return SimpleNamespace(returncode=0, stdout=rows(texts, changed), stderr=b'')
            return run
        def surfaces(run, excused):
            with patch.object(gcov_measure, 'verify_prefix'), patch.object(gcov_measure, 'clear'), \
                 patch.object(gcov_measure, 'counters', return_value={('f',): 1}), patch.object(gcov_measure.subprocess, 'run', side_effect=run):
                return gcov_measure.surfaces(Path('/prefix'), Path('/p'), Path('/out'), Path('/pin'), 'kc', None, excused=excused)
        excused = frozenset({'residual'})
        result = surfaces(run_with(subject_changed=('residual',)), excused)[3]
        self.assertEqual(result, [(group, 3, 2, 1, ['residual']) for group in ('sentence', 'ties', 'corpus')])
        with self.assertRaisesRegex(RuntimeError, r"sentence: undeclared kc surface difference .*'plain' \(pin/Rust\)"):
            surfaces(run_with(subject_changed=('plain',)), excused)
        with self.assertRaisesRegex(RuntimeError, r"'other' \(measurement\)"):
            surfaces(run_with(measured_changed=('other',)), excused)
        # Without `excused` (the bdb ledger) the original four-field result is unchanged.
        result = surfaces(run_with(subject_changed=('plain',)), None)[3]
        self.assertEqual(result, [(group, 3, 2, 1) for group in ('sentence', 'ties', 'corpus')])

    def test_delta_measurement_excuses_only_the_declared_residuals(self):
        import delta_ledger
        captured = {}
        def surfaces(*args, **kwargs):
            captured.update(kwargs)
            return defaultdict(set), set(), set(), []
        stage = lambda *a, **k: (defaultdict(set), set(), set(), [])
        args = SimpleNamespace(cell='kc', measurement_build=Path('/m'), measurement_config=Path('/c/config.h'), pinyin_so=Path('/p'),
                               zhuyin_so=Path('/z'), surface_pinyin_so=None, rust_target=Path('/t'), export=Path('/e'), model=Path('/mo'),
                               pin=Path('/pin'), oracle=Path('/o'))
        ledger_stub = SimpleNamespace(run=lambda *a: '', REPO=Path('.'), symbol_base=ledger.symbol_base, witness=lambda *a: {})
        with patch.object(gcov_measure, 'build'), patch.object(gcov_measure, 'configured_libraries', return_value=['-lkyotocabinet']), \
             patch.object(gcov_measure, 'contracts', side_effect=stage), patch.object(gcov_measure, 'gates', return_value='gate'), \
             patch.object(gcov_measure, 'same_data', side_effect=stage), patch.object(gcov_measure, 'user_store', side_effect=stage), \
             patch.object(gcov_measure, 'surfaces', side_effect=surfaces), patch.object(delta_ledger, 'emitted_inventory', return_value=set()), \
             patch.object(gcov_measure, 'limit_roots', return_value=(set(), {}, {}, {})):
            delta_ledger.measure_cell(args, ledger_stub, dict(nodes={}, roots={}, edges=set()), None)
        self.assertEqual(captured['excused'], gcov_measure.declared_residuals())

    def test_registered_measurement_limits_are_never_credited(self):
        import coverage_reviews, delta_ledger
        self.assertEqual(set(coverage_reviews.MEASUREMENT_LIMITS), {'user-gram-duplicate-export', 'user-gram-unsorted-export'})
        behavior = defaultdict(set, {('f',): {'contract/user-gram-unsorted-export', 'contract/plain'},
                                     ('g',): {'contract/user-gram-duplicate-export'}})
        contracts = (behavior, set(), set(), [('user-gram-unsorted-export', 'MATCH'), ('user-gram-duplicate-export', 'MEASUREMENT-DIFFER'),
                                             ('plain', 'MATCH'), ('broken', 'DIFFER')])
        stage = lambda *a, **k: (defaultdict(set), set(), set(), [])
        seen = []
        def align(nodes, evidence, symbol_base):
            seen.append({key: set(value) for key, value in evidence.items()})
            return {}, set()
        args = SimpleNamespace(cell='tkrzw', measurement_build=Path('/m'), measurement_config=Path('/c/config.h'), pinyin_so=Path('/p'),
                               zhuyin_so=Path('/z'), surface_pinyin_so=None, rust_target=Path('/t'), export=Path('/e'), model=Path('/mo'),
                               pin=Path('/pin'), oracle=Path('/o'))
        ledger_stub = SimpleNamespace(run=lambda *a: '', REPO=Path('.'), symbol_base=ledger.symbol_base, witness=lambda *a: {})
        with patch.object(gcov_measure, 'build'), patch.object(gcov_measure, 'configured_libraries', return_value=['-ltkrzw']), \
             patch.object(gcov_measure, 'contracts', return_value=contracts), patch.object(gcov_measure, 'gates', return_value='gate'), \
             patch.object(gcov_measure, 'same_data', side_effect=stage), patch.object(gcov_measure, 'user_store', side_effect=stage), \
             patch.object(gcov_measure, 'surfaces', side_effect=stage), patch.object(delta_ledger, 'emitted_inventory', return_value=set()), \
             patch.object(delta_ledger, 'align', side_effect=align), \
             patch.object(gcov_measure, 'limit_roots', return_value=(set(), {}, {'user-gram-unsorted-export': 'MATCH', 'broken': 'MATCH'},
                                                                      {('h',): {'contract/user-gram-unsorted-export'}, ('k',): {'contract/broken'}})):
            measured = delta_ledger.measure_cell(args, ledger_stub, dict(nodes={}, roots={}, edges=set()), None)
        self.assertEqual(dict(measured['records']['contract']), {'user-gram-unsorted-export': 'MEASUREMENT-DIFFER',
                                                                 'user-gram-duplicate-export': 'MEASUREMENT-DIFFER', 'plain': 'MATCH', 'broken': 'MATCH'})
        self.assertTrue({'contract/user-gram-unsorted-export', 'contract/user-gram-duplicate-export'} <= measured['incomplete'])
        executed_evidence = seen[1]
        self.assertEqual(executed_evidence, {('f',): {'contract/plain'}, ('k',): {'contract/broken'}})

    def test_measurement_limits_are_not_parity_findings(self):
        import delta_ledger
        records = {'contract': [('a', 'MATCH'), ('b', 'DIFFER'), ('c', 'MEASUREMENT-DIFFER'), ('d', 'TIMEOUT'), ('e', 'SKIP')],
                   'same-data': [('x', 'DIFFER'), ('y', 'MATCH')], 'user-store': [('rt', 'MEASUREMENT-DIFFER')],
                   'surface': [('ties', 3, 2, 1, ['residual'])]}
        differ, limited = delta_ledger.split_verdicts(records)
        self.assertEqual(differ, ['contract/b', 'same-data/x'])
        self.assertEqual(limited, ['contract/c (MEASUREMENT-DIFFER)', 'contract/d (TIMEOUT)', 'user-store/rt (MEASUREMENT-DIFFER)'])
        self.assertEqual(delta_ledger.split_verdicts({'contract': [('a', 'MATCH')], 'surface': []}), ([], []))

    def test_delta_rows_parse_escaped_ledger_identities(self):
        import delta_ledger
        line = ('| function/direct | [src/storage/ngram.cpp:5](https://example/ngram.cpp#L5) | pinyin::f(a&#124;b) | c:@F@f#&#124;# | pinyin_init | '
                'pinyin_init() @ src/pinyin.cpp:1 (root) | BEHAVIOR-COVERED | contract/x | baseline-962; reason |\n'
                '| external boundary | [src/pinyin.cpp:1](https://example) | malloc | LLVM-symbol:malloc | pinyin_init | path | BOUNDARY |  | reason |\n')
        rows = delta_ledger.parse_rows('# Axis J: Berkeley DB internal coverage ledger\n\n' + line)
        self.assertEqual(list(rows), ['c:@F@f#|#'])
        self.assertEqual(rows['c:@F@f#|#']['signature'], 'pinyin::f(a|b)')
        self.assertEqual((rows['c:@F@f#|#']['file'], rows['c:@F@f#|#']['line']), ('src/storage/ngram.cpp', 5))
        with self.assertRaisesRegex(RuntimeError, 'duplicate'):
            delta_ledger.parse_rows(line + line)

    def test_prefix_provenance_records_on_generation_and_compares_on_check(self):
        import delta_ledger
        with tempfile.TemporaryDirectory() as temp:
            prefix = Path(temp) / 'prefix'; (prefix / 'lib').mkdir(parents=True)
            (prefix / 'lib/libzhuyin.so').write_bytes(b'zhuyin kc')
            (prefix / 'oracle-pin.txt').write_text('shared_object_sha256=' + 'a' * 64 + '\n')
            output = Path(temp) / 'ledger.md'
            args = SimpleNamespace(cell='kc', check=False, oracle=prefix, output=output)
            computed = hashlib.sha256(b'zhuyin kc').hexdigest()
            self.assertEqual(delta_ledger.prefix_provenance(args), {'libpinyin': 'a' * 64, 'libzhuyin': computed})
            output.write_text(f'Pin `{gcov_measure.PIN}`\nOracle shared-object SHA-256: libpinyin=`{"b" * 64}`; libzhuyin=`{computed}`\n')
            args.check = True
            self.assertEqual(delta_ledger.prefix_provenance(args), {'libpinyin': 'b' * 64, 'libzhuyin': computed})
            output.write_text('no provenance here\n')
            with self.assertRaisesRegex(RuntimeError, 'does not record'):
                delta_ledger.prefix_provenance(args)

    def test_review_rules_reload_in_place_and_cell_scoped_identities_stay_apart(self):
        import coverage_reviews
        before = dict(coverage_reviews.REVIEWS)
        identities = dict(coverage_reviews.IDENTITY_REVIEWS)
        registry = coverage_reviews.REVIEWS
        coverage_reviews.reload_rules()
        self.assertIs(coverage_reviews.REVIEWS, registry)
        self.assertEqual(coverage_reviews.REVIEWS, before)
        self.assertEqual(coverage_reviews.IDENTITY_REVIEWS, identities)
        self.assertEqual(sum(status == 'ABSENT_INLINE' for status, _, _ in coverage_reviews.IDENTITY_REVIEWS.values()), 15)
        # A batch first seen at reload time (written during a measurement) is
        # executed exactly once, never imported and then reloaded.
        import sys
        fresh = 'coverage_review_rules.dbm_callback_lifetimes'
        del sys.modules[fresh]
        coverage_reviews.reload_rules()
        self.assertEqual(coverage_reviews.REVIEWS, before)
        self.assertIn(fresh, sys.modules)
        coverage_reviews.identity_review('signature:probe', [], 'probe', 'ABSENT_INLINE', cell='kc')
        try:
            self.assertNotIn('signature:probe', coverage_reviews.IDENTITY_REVIEWS)
            self.assertIn('signature:probe', coverage_reviews.CELL_IDENTITY_REVIEWS['kc'])
            with self.assertRaises(ValueError):
                coverage_reviews.identity_review('signature:probe', [], 'probe', 'ABSENT_INLINE', cell='kc')
        finally:
            coverage_reviews.reload_rules()
        self.assertNotIn('signature:probe', coverage_reviews.CELL_IDENTITY_REVIEWS['kc'])

    def test_known_defect_rows_are_never_credited_and_drop_narrowed_evidence(self):
        import coverage_reviews, delta_ledger
        site = ('src/storage/chewing_large_table2_kyotodb.cpp', 459)
        self.assertEqual(coverage_reviews.KNOWN_DEFECTS[site]['issue'], '#785')
        self.assertEqual(coverage_reviews.KNOWN_DEFECTS[site]['excluded'], ('contract/issue784-', 'contract/issue762-'))
        self.assertEqual(len(coverage_reviews.KNOWN_DEFECTS), 4)
        node = dict(file=site[0], line=site[1], signature='visit_full()', usr='c:probe', name='visit_full')
        executed = {'defect': {'contract/issue784-index-keys-mask-lookup-pinyin', 'contract/issue762-token-reuse-x', 'contract/abort-mask-out-empty-index-key', 'user-store/four-phase-round-trip'}}
        measured = dict(executed=executed, unknown_paths={}, compiled={'defect'}, failed_nodes={})
        category, mapping = delta_ledger.classify(SimpleNamespace(MAPPINGS={}, REPO=Path('.')), 'kc', 'defect', node, measured)
        self.assertEqual(category, 'KNOWN-DEFECT')
        self.assertIn('#785', mapping)
        self.assertIn('2 narrowed matching case(s)', mapping)
        self.assertEqual(delta_ledger.evidence('defect', node, executed), {'contract/abort-mask-out-empty-index-key', 'user-store/four-phase-round-trip'})
        other = dict(node, line=460)
        self.assertEqual(delta_ledger.evidence('defect', other, executed), executed['defect'])
        with self.assertRaises(ValueError):
            coverage_reviews.known_defect(site[0], [site[1]], '#785', (), 'again')
        coverage_reviews.reload_rules()
        self.assertEqual(len(coverage_reviews.KNOWN_DEFECTS), 4)


if __name__ == "__main__":
    unittest.main()
